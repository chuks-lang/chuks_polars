// chuks_polars_shim — C-ABI bridge around the `polars` Rust crate.
//
// v0.2 — full data-engineering surface. All `*mut DataFrame` handles are
// produced via Box::into_raw and freed by re-boxing in `pl_free_df`.
// All `*mut c_char` strings returned to Chuks must be released via
// `pl_free_string`.
//
// Convention for list-style parameters: comma-separated strings.
//   cols_csv = "a,b,c"
//   desc_csv = "1,0,1"
//   pairs_csv = "old1=new1,old2=new2"
//   agg_specs = "col:op,col:op"  (op in sum,mean,min,max,count,std,
//                                  median,first,last,n_unique,count_distinct)
//   how       = "inner" | "left" | "outer" | "full" | "cross" | "semi" | "anti"

use polars::prelude::*;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::path::PathBuf;
use std::ptr;

#[inline]
unsafe fn cstr_to_str<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        None
    } else {
        CStr::from_ptr(p).to_str().ok()
    }
}

#[inline]
fn ret_df(df: PolarsResult<DataFrame>) -> *mut DataFrame {
    match df {
        Ok(d) => Box::into_raw(Box::new(d)),
        Err(_) => ptr::null_mut(),
    }
}

#[inline]
fn ret_cstr(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(c) => c.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

#[inline]
fn split_csv(s: &str) -> Vec<String> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split(',').map(|t| t.trim().to_string()).collect()
}

// Best-effort literal parse for filter values:
//   "true"/"false" -> bool, then i64, then f64, else string.
fn parse_literal(v: &str) -> Expr {
    let t = v.trim();
    match t {
        "true" => return lit(true),
        "false" => return lit(false),
        _ => {}
    }
    if let Ok(i) = t.parse::<i64>() {
        return lit(i);
    }
    if let Ok(f) = t.parse::<f64>() {
        return lit(f);
    }
    lit(t.to_string())
}

// ── I/O ─────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn pl_read_csv(path: *const c_char) -> *mut DataFrame {
    let s = match unsafe { cstr_to_str(path) } {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let pb = PathBuf::from(s);
    ret_df(
        CsvReadOptions::default()
            .with_has_header(true)
            .try_into_reader_with_file_path(Some(pb))
            .and_then(|r| r.finish()),
    )
}

// has_header: 0=no, 1=yes. skip_rows: rows to skip BEFORE header.
// n_rows: -1 = all. separator: single byte (ASCII).
#[no_mangle]
pub extern "C" fn pl_read_csv_opts(
    path: *const c_char,
    sep: u8,
    has_header: c_int,
    skip_rows: c_int,
    n_rows: c_int,
) -> *mut DataFrame {
    let s = match unsafe { cstr_to_str(path) } {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let pb = PathBuf::from(s);
    let mut opts = CsvReadOptions::default()
        .with_has_header(has_header != 0)
        .with_skip_rows(if skip_rows < 0 { 0 } else { skip_rows as usize });
    if n_rows >= 0 {
        opts = opts.with_n_rows(Some(n_rows as usize));
    }
    let parse = CsvParseOptions::default().with_separator(if sep == 0 { b',' } else { sep });
    opts = opts.with_parse_options(parse);
    ret_df(
        opts.try_into_reader_with_file_path(Some(pb))
            .and_then(|r| r.finish()),
    )
}

#[no_mangle]
pub extern "C" fn pl_read_parquet(path: *const c_char) -> *mut DataFrame {
    let s = match unsafe { cstr_to_str(path) } {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let f = match std::fs::File::open(s) {
        Ok(f) => f,
        Err(_) => return ptr::null_mut(),
    };
    ret_df(ParquetReader::new(f).finish())
}

#[no_mangle]
pub extern "C" fn pl_write_csv(df: *mut DataFrame, path: *const c_char) -> c_int {
    if df.is_null() {
        return -1;
    }
    let s = match unsafe { cstr_to_str(path) } {
        Some(s) => s,
        None => return -1,
    };
    let df_ref = unsafe { &mut *df };
    let f = match std::fs::File::create(s) {
        Ok(f) => f,
        Err(_) => return -1,
    };
    match CsvWriter::new(f).finish(df_ref) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

#[no_mangle]
pub extern "C" fn pl_write_parquet(df: *mut DataFrame, path: *const c_char) -> c_int {
    if df.is_null() {
        return -1;
    }
    let s = match unsafe { cstr_to_str(path) } {
        Some(s) => s,
        None => return -1,
    };
    let df_ref = unsafe { &mut *df };
    let f = match std::fs::File::create(s) {
        Ok(f) => f,
        Err(_) => return -1,
    };
    match ParquetWriter::new(f).finish(df_ref) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

// ── Inspection ──────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn pl_height(df: *mut DataFrame) -> i64 {
    if df.is_null() {
        return -1;
    }
    unsafe { (&*df).height() as i64 }
}

#[no_mangle]
pub extern "C" fn pl_width(df: *mut DataFrame) -> i64 {
    if df.is_null() {
        return -1;
    }
    unsafe { (&*df).width() as i64 }
}

#[no_mangle]
pub extern "C" fn pl_columns_csv(df: *mut DataFrame) -> *mut c_char {
    if df.is_null() {
        return ptr::null_mut();
    }
    let df_ref = unsafe { &*df };
    let names: Vec<String> = df_ref
        .get_column_names()
        .iter()
        .map(|n| n.to_string())
        .collect();
    ret_cstr(names.join(","))
}

#[no_mangle]
pub extern "C" fn pl_dtypes_csv(df: *mut DataFrame) -> *mut c_char {
    if df.is_null() {
        return ptr::null_mut();
    }
    let df_ref = unsafe { &*df };
    let dtypes: Vec<String> = df_ref.dtypes().iter().map(|d| format!("{}", d)).collect();
    ret_cstr(dtypes.join(","))
}

// Returns "col1:0,col2:3,col3:1" — null count per column.
#[no_mangle]
pub extern "C" fn pl_null_counts_csv(df: *mut DataFrame) -> *mut c_char {
    if df.is_null() {
        return ptr::null_mut();
    }
    let df_ref = unsafe { &*df };
    let parts: Vec<String> = df_ref
        .get_columns()
        .iter()
        .map(|c| format!("{}:{}", c.name(), c.null_count()))
        .collect();
    ret_cstr(parts.join(","))
}

// ── Selection / shape ───────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn pl_select(df: *mut DataFrame, cols_csv: *const c_char) -> *mut DataFrame {
    if df.is_null() {
        return ptr::null_mut();
    }
    let cols = match unsafe { cstr_to_str(cols_csv) } {
        Some(s) => split_csv(s),
        None => return ptr::null_mut(),
    };
    let df_ref = unsafe { &*df };
    ret_df(df_ref.select(cols.iter().map(|s| s.as_str())))
}

#[no_mangle]
pub extern "C" fn pl_drop(df: *mut DataFrame, cols_csv: *const c_char) -> *mut DataFrame {
    if df.is_null() {
        return ptr::null_mut();
    }
    let cols = match unsafe { cstr_to_str(cols_csv) } {
        Some(s) => split_csv(s),
        None => return ptr::null_mut(),
    };
    let df_ref = unsafe { &*df };
    let exprs: Vec<Expr> = cols.iter().map(|c| col(c.as_str())).collect();
    ret_df(df_ref.clone().lazy().drop(exprs).collect())
}

#[no_mangle]
pub extern "C" fn pl_rename(df: *mut DataFrame, pairs_csv: *const c_char) -> *mut DataFrame {
    if df.is_null() {
        return ptr::null_mut();
    }
    let pairs = match unsafe { cstr_to_str(pairs_csv) } {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let df_ref = unsafe { &*df };
    let mut out = df_ref.clone();
    for pair in pairs.split(',') {
        let mut sp = pair.splitn(2, '=');
        let (old, new) = match (sp.next(), sp.next()) {
            (Some(a), Some(b)) => (a.trim(), b.trim()),
            _ => return ptr::null_mut(),
        };
        if out.rename(old, new.into()).is_err() {
            return ptr::null_mut();
        }
    }
    Box::into_raw(Box::new(out))
}

#[no_mangle]
pub extern "C" fn pl_head(df: *mut DataFrame, n: i32) -> *mut DataFrame {
    if df.is_null() {
        return ptr::null_mut();
    }
    let n_take = if n < 0 { 5usize } else { n as usize };
    let df_ref = unsafe { &*df };
    Box::into_raw(Box::new(df_ref.head(Some(n_take))))
}

#[no_mangle]
pub extern "C" fn pl_tail(df: *mut DataFrame, n: i32) -> *mut DataFrame {
    if df.is_null() {
        return ptr::null_mut();
    }
    let n_take = if n < 0 { 5usize } else { n as usize };
    let df_ref = unsafe { &*df };
    Box::into_raw(Box::new(df_ref.tail(Some(n_take))))
}

#[no_mangle]
pub extern "C" fn pl_slice(df: *mut DataFrame, offset: i64, length: i64) -> *mut DataFrame {
    if df.is_null() {
        return ptr::null_mut();
    }
    let df_ref = unsafe { &*df };
    let len = if length < 0 { df_ref.height() } else { length as usize };
    Box::into_raw(Box::new(df_ref.slice(offset, len)))
}

#[no_mangle]
pub extern "C" fn pl_sample_n(df: *mut DataFrame, n: i64, seed: i64) -> *mut DataFrame {
    if df.is_null() {
        return ptr::null_mut();
    }
    let df_ref = unsafe { &*df };
    let seed_opt = if seed < 0 { None } else { Some(seed as u64) };
    ret_df(df_ref.sample_n_literal(n as usize, false, true, seed_opt))
}

// keep: "first"|"last"|"any"|"none". empty subset_csv = all columns.
#[no_mangle]
pub extern "C" fn pl_unique(
    df: *mut DataFrame,
    subset_csv: *const c_char,
    keep: *const c_char,
) -> *mut DataFrame {
    if df.is_null() {
        return ptr::null_mut();
    }
    let subset_s = match unsafe { cstr_to_str(subset_csv) } {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let keep_s = match unsafe { cstr_to_str(keep) } {
        Some(s) => s,
        None => "any",
    };
    let strategy = match keep_s {
        "first" => UniqueKeepStrategy::First,
        "last" => UniqueKeepStrategy::Last,
        "none" => UniqueKeepStrategy::None,
        _ => UniqueKeepStrategy::Any,
    };
    let df_ref = unsafe { &*df };
    let subset: Option<Vec<String>> = if subset_s.is_empty() {
        None
    } else {
        Some(split_csv(subset_s))
    };
    let result = match subset.as_ref() {
        Some(v) => df_ref.unique::<&[String], String>(Some(v.as_slice()), strategy, None),
        None => df_ref.unique::<&[String], String>(None, strategy, None),
    };
    ret_df(result)
}

// ── Sort ────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn pl_sort_by(
    df: *mut DataFrame,
    cols_csv: *const c_char,
    desc_csv: *const c_char,
) -> *mut DataFrame {
    if df.is_null() {
        return ptr::null_mut();
    }
    let cols_s = match unsafe { cstr_to_str(cols_csv) } {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let desc_s = match unsafe { cstr_to_str(desc_csv) } {
        Some(s) => s,
        None => "",
    };
    let cols = split_csv(cols_s);
    if cols.is_empty() {
        return ptr::null_mut();
    }
    let desc_vec: Vec<bool> = if desc_s.is_empty() {
        vec![false; cols.len()]
    } else {
        split_csv(desc_s)
            .iter()
            .map(|t| t == "1" || t == "true")
            .chain(std::iter::repeat(false))
            .take(cols.len())
            .collect()
    };
    let df_ref = unsafe { &*df };
    let opts = SortMultipleOptions::default()
        .with_order_descending_multi(desc_vec)
        .with_nulls_last(true);
    ret_df(df_ref.sort(cols, opts))
}

// ── Filter ──────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn pl_filter(
    df: *mut DataFrame,
    col_name: *const c_char,
    op: *const c_char,
    value: *const c_char,
) -> *mut DataFrame {
    if df.is_null() {
        return ptr::null_mut();
    }
    let c_name = match unsafe { cstr_to_str(col_name) } {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let op_s = match unsafe { cstr_to_str(op) } {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let v = match unsafe { cstr_to_str(value) } {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let lhs = col(c_name);
    let rhs = parse_literal(v);
    let predicate = match op_s {
        "eq" | "==" => lhs.eq(rhs),
        "ne" | "!=" => lhs.neq(rhs),
        "gt" | ">" => lhs.gt(rhs),
        "lt" | "<" => lhs.lt(rhs),
        "gte" | ">=" => lhs.gt_eq(rhs),
        "lte" | "<=" => lhs.lt_eq(rhs),
        _ => return ptr::null_mut(),
    };
    let df_ref = unsafe { &*df };
    ret_df(df_ref.clone().lazy().filter(predicate).collect())
}

#[no_mangle]
pub extern "C" fn pl_filter_is_null(
    df: *mut DataFrame,
    col_name: *const c_char,
) -> *mut DataFrame {
    if df.is_null() {
        return ptr::null_mut();
    }
    let c_name = match unsafe { cstr_to_str(col_name) } {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let df_ref = unsafe { &*df };
    ret_df(df_ref.clone().lazy().filter(col(c_name).is_null()).collect())
}

#[no_mangle]
pub extern "C" fn pl_filter_not_null(
    df: *mut DataFrame,
    col_name: *const c_char,
) -> *mut DataFrame {
    if df.is_null() {
        return ptr::null_mut();
    }
    let c_name = match unsafe { cstr_to_str(col_name) } {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let df_ref = unsafe { &*df };
    ret_df(
        df_ref
            .clone()
            .lazy()
            .filter(col(c_name).is_not_null())
            .collect(),
    )
}

// ── Null handling ───────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn pl_drop_nulls(df: *mut DataFrame) -> *mut DataFrame {
    if df.is_null() {
        return ptr::null_mut();
    }
    let df_ref = unsafe { &*df };
    ret_df(df_ref.clone().lazy().drop_nulls(None).collect())
}

#[no_mangle]
pub extern "C" fn pl_drop_nulls_in(
    df: *mut DataFrame,
    cols_csv: *const c_char,
) -> *mut DataFrame {
    if df.is_null() {
        return ptr::null_mut();
    }
    let cols_s = match unsafe { cstr_to_str(cols_csv) } {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let cols = split_csv(cols_s);
    if cols.is_empty() {
        return ptr::null_mut();
    }
    let df_ref = unsafe { &*df };
    let exprs: Vec<Expr> = cols.iter().map(|c| col(c.as_str())).collect();
    ret_df(df_ref.clone().lazy().drop_nulls(Some(exprs)).collect())
}

// ── GroupBy ─────────────────────────────────────────────────────────

// agg_specs: "col:op,col:op,..." op in [sum, mean, min, max, count, std,
// median, first, last, n_unique, count_distinct]. Output column names are
// "<col>_<op>" (so "amount:sum" -> "amount_sum") to avoid collisions.
#[no_mangle]
pub extern "C" fn pl_group_by_agg(
    df: *mut DataFrame,
    by_csv: *const c_char,
    agg_specs: *const c_char,
) -> *mut DataFrame {
    if df.is_null() {
        return ptr::null_mut();
    }
    let by_s = match unsafe { cstr_to_str(by_csv) } {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let aggs_s = match unsafe { cstr_to_str(agg_specs) } {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let by = split_csv(by_s);
    if by.is_empty() {
        return ptr::null_mut();
    }
    let mut aggs: Vec<Expr> = Vec::new();
    for spec in aggs_s.split(',') {
        let mut sp = spec.splitn(2, ':');
        let (cname, op) = match (sp.next(), sp.next()) {
            (Some(a), Some(b)) => (a.trim(), b.trim()),
            _ => return ptr::null_mut(),
        };
        let alias = format!("{}_{}", cname, op);
        let e = match op {
            "sum" => col(cname).sum(),
            "mean" | "avg" => col(cname).mean(),
            "min" => col(cname).min(),
            "max" => col(cname).max(),
            "count" => col(cname).count(),
            "std" => col(cname).std(1),
            "var" => col(cname).var(1),
            "median" => col(cname).median(),
            "first" => col(cname).first(),
            "last" => col(cname).last(),
            "n_unique" | "count_distinct" => col(cname).n_unique(),
            _ => return ptr::null_mut(),
        }
        .alias(alias.as_str());
        aggs.push(e);
    }
    let df_ref = unsafe { &*df };
    let by_exprs: Vec<Expr> = by.iter().map(|c| col(c.as_str())).collect();
    let sort_cols = by.clone();
    ret_df(
        df_ref
            .clone()
            .lazy()
            .group_by(by_exprs)
            .agg(aggs)
            .sort(sort_cols, SortMultipleOptions::default())
            .collect(),
    )
}

// ── Joins ───────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn pl_join(
    left: *mut DataFrame,
    right: *mut DataFrame,
    left_csv: *const c_char,
    right_csv: *const c_char,
    how: *const c_char,
) -> *mut DataFrame {
    if left.is_null() || right.is_null() {
        return ptr::null_mut();
    }
    let l = unsafe { &*left };
    let r = unsafe { &*right };
    let left_s = match unsafe { cstr_to_str(left_csv) } {
        Some(s) => s,
        None => "",
    };
    let right_s = match unsafe { cstr_to_str(right_csv) } {
        Some(s) => s,
        None => "",
    };
    let how_s = match unsafe { cstr_to_str(how) } {
        Some(s) => s,
        None => "inner",
    };
    let how_type = match how_s {
        "inner" => JoinType::Inner,
        "left" => JoinType::Left,
        "outer" | "full" => JoinType::Full,
        "cross" => JoinType::Cross,
        "semi" => JoinType::Semi,
        "anti" => JoinType::Anti,
        _ => return ptr::null_mut(),
    };
    let left_keys: Vec<Expr> = split_csv(left_s).into_iter().map(|c| col(c.as_str())).collect();
    let right_keys: Vec<Expr> = split_csv(right_s)
        .into_iter()
        .map(|c| col(c.as_str()))
        .collect();
    ret_df(
        l.clone()
            .lazy()
            .join(
                r.clone().lazy(),
                left_keys,
                right_keys,
                JoinArgs::new(how_type),
            )
            .collect(),
    )
}

// ── Column scalars ──────────────────────────────────────────────────

// Collect a single-cell f64 from a single-column aggregation. Returns 1
// on success (*out set), 0 on null, -1 on error.
fn col_scalar_f64(df: &DataFrame, col_name: &str, agg: Expr, out: *mut f64) -> c_int {
    let res = df
        .clone()
        .lazy()
        .select([agg.alias("v")])
        .collect();
    let res = match res {
        Ok(d) => d,
        Err(_) => return -1,
    };
    let s = match res.column("v") {
        Ok(c) => c,
        Err(_) => return -1,
    };
    let v = match s.get(0) {
        Ok(v) => v,
        Err(_) => return -1,
    };
    let f = match v {
        AnyValue::Null => return 0,
        AnyValue::Float64(x) => x,
        AnyValue::Float32(x) => x as f64,
        AnyValue::Int64(x) => x as f64,
        AnyValue::Int32(x) => x as f64,
        AnyValue::UInt64(x) => x as f64,
        AnyValue::UInt32(x) => x as f64,
        AnyValue::Int16(x) => x as f64,
        AnyValue::UInt16(x) => x as f64,
        AnyValue::Int8(x) => x as f64,
        AnyValue::UInt8(x) => x as f64,
        _ => return -1,
    };
    if !out.is_null() {
        unsafe { *out = f };
    }
    let _ = col_name; // silence unused
    1
}

#[no_mangle]
pub extern "C" fn pl_col_sum_f64(
    df: *mut DataFrame,
    col_name: *const c_char,
    out: *mut f64,
) -> c_int {
    if df.is_null() {
        return -1;
    }
    let c = match unsafe { cstr_to_str(col_name) } {
        Some(s) => s,
        None => return -1,
    };
    col_scalar_f64(unsafe { &*df }, c, col(c).sum(), out)
}

#[no_mangle]
pub extern "C" fn pl_col_mean_f64(
    df: *mut DataFrame,
    col_name: *const c_char,
    out: *mut f64,
) -> c_int {
    if df.is_null() {
        return -1;
    }
    let c = match unsafe { cstr_to_str(col_name) } {
        Some(s) => s,
        None => return -1,
    };
    col_scalar_f64(unsafe { &*df }, c, col(c).mean(), out)
}

#[no_mangle]
pub extern "C" fn pl_col_min_f64(
    df: *mut DataFrame,
    col_name: *const c_char,
    out: *mut f64,
) -> c_int {
    if df.is_null() {
        return -1;
    }
    let c = match unsafe { cstr_to_str(col_name) } {
        Some(s) => s,
        None => return -1,
    };
    col_scalar_f64(unsafe { &*df }, c, col(c).min(), out)
}

#[no_mangle]
pub extern "C" fn pl_col_max_f64(
    df: *mut DataFrame,
    col_name: *const c_char,
    out: *mut f64,
) -> c_int {
    if df.is_null() {
        return -1;
    }
    let c = match unsafe { cstr_to_str(col_name) } {
        Some(s) => s,
        None => return -1,
    };
    col_scalar_f64(unsafe { &*df }, c, col(c).max(), out)
}

#[no_mangle]
pub extern "C" fn pl_col_count(df: *mut DataFrame, col_name: *const c_char) -> i64 {
    if df.is_null() {
        return -1;
    }
    let c = match unsafe { cstr_to_str(col_name) } {
        Some(s) => s,
        None => return -1,
    };
    let df_ref = unsafe { &*df };
    match df_ref.column(c) {
        Ok(s) => (s.len() - s.null_count()) as i64,
        Err(_) => -1,
    }
}

// ── Row access ──────────────────────────────────────────────────────

// Returns the cell at (col, row) formatted as a string. NULL on error.
// Empty string represents a null value.
#[no_mangle]
pub extern "C" fn pl_get_str(
    df: *mut DataFrame,
    col_name: *const c_char,
    row: i64,
) -> *mut c_char {
    if df.is_null() || row < 0 {
        return ptr::null_mut();
    }
    let c = match unsafe { cstr_to_str(col_name) } {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let df_ref = unsafe { &*df };
    let s = match df_ref.column(c) {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };
    if row as usize >= s.len() {
        return ptr::null_mut();
    }
    let v = match s.get(row as usize) {
        Ok(v) => v,
        Err(_) => return ptr::null_mut(),
    };
    let formatted = match v {
        AnyValue::Null => String::new(),
        AnyValue::String(x) => x.to_string(),
        AnyValue::StringOwned(x) => x.to_string(),
        other => format!("{}", other),
    };
    ret_cstr(formatted)
}

// ── Concat ──────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn pl_vstack(top: *mut DataFrame, bottom: *mut DataFrame) -> *mut DataFrame {
    if top.is_null() || bottom.is_null() {
        return ptr::null_mut();
    }
    let t = unsafe { &*top };
    let b = unsafe { &*bottom };
    ret_df(t.vstack(b))
}

// ── Bulk column read ────────────────────────────────────────────────
//
// Returns the entire column as a single C string with the byte `sep`
// inserted between successive values. The caller knows the row count
// (`pl_height`) and the separator, so no per-cell FFI crossing is
// needed — 1M cells become 1 call.
//
// Null cells encode as an empty value (two consecutive separators).
// Sep `0` is invalid (the buffer is a C-string, so internal NULs would
// truncate it on read); callers should pass a byte that cannot appear
// in the underlying data — `0x01` (SOH) is recommended for text data,
// and is what the Chuks `DataFrame.column(name): []string` wrapper uses.
//
// On error (null df / col / unknown column / sep == 0) returns NULL.
#[no_mangle]
pub extern "C" fn pl_col_str(
    df: *mut DataFrame,
    col_name: *const c_char,
    sep: u8,
) -> *mut c_char {
    if df.is_null() || sep == 0 {
        return ptr::null_mut();
    }
    let cname = match unsafe { cstr_to_str(col_name) } {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let df_ref = unsafe { &*df };
    let s = match df_ref.column(cname) {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };
    let n = s.len();
    let sep_ch = sep as char;

    // Pre-size the buffer. For string dtypes we know exact byte length;
    // for numeric dtypes we estimate ~16 bytes/value (more than enough
    // for f64 / i64 prints; the buffer will only grow once if exceeded).
    let mut buf = String::with_capacity(n.saturating_mul(16));

    // Fast path for string-typed columns: AnyValue::String borrows the
    // underlying &str so we avoid the per-cell `format!` allocation that
    // pl_get_str pays.
    if matches!(s.dtype(), DataType::String) {
        match s.str() {
            Ok(ca) => {
                for (i, v) in ca.into_iter().enumerate() {
                    if i > 0 {
                        buf.push(sep_ch);
                    }
                    if let Some(x) = v {
                        buf.push_str(x);
                    }
                }
            }
            Err(_) => return ptr::null_mut(),
        }
        return ret_cstr(buf);
    }

    // Numeric / bool / categorical / etc. — fall back to AnyValue
    // formatting. Still a single FFI call instead of `n` calls.
    use std::fmt::Write;
    for i in 0..n {
        if i > 0 {
            buf.push(sep_ch);
        }
        let v = match s.get(i) {
            Ok(v) => v,
            Err(_) => return ptr::null_mut(),
        };
        match v {
            AnyValue::Null => {}
            AnyValue::String(x) => buf.push_str(x),
            AnyValue::StringOwned(x) => buf.push_str(x.as_str()),
            other => {
                // `write!` into a String is infallible.
                let _ = write!(buf, "{}", other);
            }
        }
    }
    ret_cstr(buf)
}

// ── Fast numeric column extractor ──────────────────────────────────
// Writes column values cast to f64 directly into the caller-provided
// buffer. Avoids the per-cell string allocation that pl_col_str / Chuks
// `float(str)` parsing pays. Nulls are encoded as NaN.
//
// Buffer convention: `out_buf` must point to at least `out_cap` bytes,
// where `out_cap >= n_rows * 8`. Returns the number of f64 elements
// written (== n_rows) on success, or -1 on error (null df / unknown col /
// non-numeric dtype / insufficient capacity).
#[no_mangle]
pub extern "C" fn pl_col_f64(
    df: *mut DataFrame,
    col_name: *const c_char,
    out_buf: *mut f64,
    out_cap: i64,
) -> i64 {
    if df.is_null() || out_buf.is_null() || out_cap < 0 {
        return -1;
    }
    let cname = match unsafe { cstr_to_str(col_name) } {
        Some(s) => s,
        None => return -1,
    };
    let df_ref = unsafe { &*df };
    let s = match df_ref.column(cname) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let n = s.len();
    let need = (n as i64).saturating_mul(8);
    if out_cap < need {
        return -1;
    }
    let casted = match s.cast(&DataType::Float64) {
        Ok(c) => c,
        Err(_) => return -1,
    };
    let ca = match casted.f64() {
        Ok(c) => c,
        Err(_) => return -1,
    };
    let dst = unsafe { std::slice::from_raw_parts_mut(out_buf, n) };
    // Fast path: contiguous, no nulls.
    if ca.null_count() == 0 {
        if let Some(slice) = ca.cont_slice().ok() {
            dst.copy_from_slice(slice);
            return n as i64;
        }
    }
    for (i, v) in ca.into_iter().enumerate() {
        dst[i] = v.unwrap_or(f64::NAN);
    }
    n as i64
}

// ── Fast multi-column matrix extractor ─────────────────────────────
// Writes a flat row-major f64 matrix of (rows × n_cols) into the
// caller-allocated buffer. `cols_csv` is a comma-separated list of
// column names; column order in the buffer matches the CSV order.
// Nulls become NaN. Returns the number of f64 elements written
// (rows * n_cols), or -1 on error.
#[no_mangle]
pub extern "C" fn pl_matrix_f64(
    df: *mut DataFrame,
    cols_csv: *const c_char,
    out_buf: *mut f64,
    out_cap: i64,
) -> i64 {
    if df.is_null() || out_buf.is_null() || out_cap < 0 {
        return -1;
    }
    let csv = match unsafe { cstr_to_str(cols_csv) } {
        Some(s) => s,
        None => return -1,
    };
    let df_ref = unsafe { &*df };
    let names = split_csv(csv);
    let nc = names.len();
    if nc == 0 {
        return 0;
    }
    let nr = df_ref.height();
    let total = (nr as i64).saturating_mul(nc as i64);
    if out_cap < total.saturating_mul(8) {
        return -1;
    }
    // Materialize each column as a contiguous f64 vec (cheap when the
    // column already has standard layout; otherwise polars walks chunks
    // exactly once). Then scatter into row-major order in one tight
    // loop with no per-cell FFI cost.
    let mut casts: Vec<Vec<f64>> = Vec::with_capacity(nc);
    for name in &names {
        let s = match df_ref.column(name) {
            Ok(s) => s,
            Err(_) => return -1,
        };
        let casted = match s.cast(&DataType::Float64) {
            Ok(c) => c,
            Err(_) => return -1,
        };
        let ca = match casted.f64() {
            Ok(c) => c,
            Err(_) => return -1,
        };
        let mut v: Vec<f64> = Vec::with_capacity(nr);
        if ca.null_count() == 0 {
            if let Ok(slice) = ca.cont_slice() {
                v.extend_from_slice(slice);
            } else {
                for x in ca.into_iter() {
                    v.push(x.unwrap_or(f64::NAN));
                }
            }
        } else {
            for x in ca.into_iter() {
                v.push(x.unwrap_or(f64::NAN));
            }
        }
        if v.len() != nr {
            return -1;
        }
        casts.push(v);
    }
    let dst = unsafe { std::slice::from_raw_parts_mut(out_buf, nr * nc) };
    for r in 0..nr {
        let row = &mut dst[r * nc..(r + 1) * nc];
        for c in 0..nc {
            row[c] = casts[c][r];
        }
    }
    total
}

// ── Lazy Series expression executor ─────────────────────────────────
// Parses a JSON plan describing a polars expression tree and executes
// it against `df`, writing the result column cast to f64 into the
// caller-provided buffer. One FFI call replaces a chain of eager
// per-op calls. Plan node shapes:
//   {"op":"col","name":"<col>"}
//   {"op":"lit","t":"f64"|"i64"|"bool","v":<value>}
//   {"op":"binop","name":"eq"|"ne"|"gt"|"lt"|"ge"|"le"
//                       |"add"|"sub"|"mul"|"div"
//                       |"and"|"or",
//                "l":<plan>,"r":<plan>}
//   {"op":"cast","dtype":"f64"|"i64"|"bool","arg":<plan>}
//   {"op":"fillnull","v":<f64>,"arg":<plan>}
//   {"op":"isnull","arg":<plan>}
//   {"op":"isnotnull","arg":<plan>}
//   {"op":"not","arg":<plan>}
//
// Returns n_elems written (== df.height()) or -1 on error.
fn expr_from_json(v: &serde_json::Value) -> Result<Expr, String> {
    let obj = v.as_object().ok_or_else(|| "plan node must be object".to_string())?;
    let op = obj
        .get("op")
        .and_then(|x| x.as_str())
        .ok_or_else(|| "missing op".to_string())?;
    match op {
        "col" => {
            let name = obj
                .get("name")
                .and_then(|x| x.as_str())
                .ok_or_else(|| "col missing name".to_string())?;
            Ok(col(name))
        }
        "lit" => {
            let t = obj
                .get("t")
                .and_then(|x| x.as_str())
                .unwrap_or("f64");
            let vv = obj.get("v").ok_or_else(|| "lit missing v".to_string())?;
            match t {
                "f64" => Ok(lit(vv.as_f64().ok_or_else(|| "lit f64 not number".to_string())?)),
                "i64" => Ok(lit(vv.as_i64().ok_or_else(|| "lit i64 not int".to_string())?)),
                "bool" => Ok(lit(vv.as_bool().ok_or_else(|| "lit bool not bool".to_string())?)),
                "str" => Ok(lit(vv
                    .as_str()
                    .ok_or_else(|| "lit str not string".to_string())?
                    .to_string())),
                _ => Err(format!("unknown lit type: {}", t)),
            }
        }
        "binop" => {
            let name = obj
                .get("name")
                .and_then(|x| x.as_str())
                .ok_or_else(|| "binop missing name".to_string())?;
            let l = expr_from_json(obj.get("l").ok_or_else(|| "binop missing l".to_string())?)?;
            let r = expr_from_json(obj.get("r").ok_or_else(|| "binop missing r".to_string())?)?;
            match name {
                // Comparison ops: cast both operands to f64 to avoid
                // dtype-strict mismatches (e.g. i64 col == f64 lit).
                "eq" => Ok(l.cast(DataType::Float64).eq(r.cast(DataType::Float64))),
                "ne" => Ok(l.cast(DataType::Float64).neq(r.cast(DataType::Float64))),
                "gt" => Ok(l.cast(DataType::Float64).gt(r.cast(DataType::Float64))),
                "lt" => Ok(l.cast(DataType::Float64).lt(r.cast(DataType::Float64))),
                "ge" => Ok(l.cast(DataType::Float64).gt_eq(r.cast(DataType::Float64))),
                "le" => Ok(l.cast(DataType::Float64).lt_eq(r.cast(DataType::Float64))),
                "add" => Ok(l + r),
                "sub" => Ok(l - r),
                "mul" => Ok(l * r),
                "div" => Ok(l / r),
                "and" => Ok(l.and(r)),
                "or" => Ok(l.or(r)),
                _ => Err(format!("unknown binop: {}", name)),
            }
        }
        "cast" => {
            let dt = obj
                .get("dtype")
                .and_then(|x| x.as_str())
                .ok_or_else(|| "cast missing dtype".to_string())?;
            let arg = expr_from_json(obj.get("arg").ok_or_else(|| "cast missing arg".to_string())?)?;
            let target = match dt {
                "f64" => DataType::Float64,
                "i64" => DataType::Int64,
                "bool" => DataType::Boolean,
                _ => return Err(format!("unknown cast dtype: {}", dt)),
            };
            Ok(arg.cast(target))
        }
        "fillnull" => {
            let vv = obj
                .get("v")
                .and_then(|x| x.as_f64())
                .ok_or_else(|| "fillnull missing v".to_string())?;
            let arg = expr_from_json(obj.get("arg").ok_or_else(|| "fillnull missing arg".to_string())?)?;
            Ok(arg.fill_null(lit(vv)))
        }
        "isnull" => {
            let arg = expr_from_json(obj.get("arg").ok_or_else(|| "isnull missing arg".to_string())?)?;
            Ok(arg.is_null())
        }
        "isnotnull" => {
            let arg = expr_from_json(obj.get("arg").ok_or_else(|| "isnotnull missing arg".to_string())?)?;
            Ok(arg.is_not_null())
        }
        "not" => {
            let arg = expr_from_json(obj.get("arg").ok_or_else(|| "not missing arg".to_string())?)?;
            Ok(arg.not())
        }
        _ => Err(format!("unknown op: {}", op)),
    }
}

#[no_mangle]
pub extern "C" fn pl_series_exec_f64(
    df: *mut DataFrame,
    plan_json: *const c_char,
    out_buf: *mut f64,
    out_cap: i64,
) -> i64 {
    if df.is_null() || out_buf.is_null() || out_cap < 0 {
        return -1;
    }
    let s = match unsafe { cstr_to_str(plan_json) } {
        Some(s) => s,
        None => return -1,
    };
    let v: serde_json::Value = match serde_json::from_str(s) {
        Ok(v) => v,
        Err(_) => return -1,
    };
    let expr = match expr_from_json(&v) {
        Ok(e) => e,
        Err(_) => return -1,
    };
    let df_ref = unsafe { &*df };
    let result = df_ref
        .clone()
        .lazy()
        .select([expr.alias("__chuks_out")])
        .collect();
    let out_df = match result {
        Ok(d) => d,
        Err(_) => return -1,
    };
    let series = match out_df.column("__chuks_out") {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let n = series.len();
    if (n as i64).saturating_mul(8) > out_cap {
        return -1;
    }
    let casted = match series.cast(&DataType::Float64) {
        Ok(c) => c,
        Err(_) => return -1,
    };
    let ca = match casted.f64() {
        Ok(c) => c,
        Err(_) => return -1,
    };
    let dst = unsafe { std::slice::from_raw_parts_mut(out_buf, n) };
    if ca.null_count() == 0 {
        if let Ok(slice) = ca.cont_slice() {
            dst.copy_from_slice(slice);
            return n as i64;
        }
    }
    for (i, x) in ca.into_iter().enumerate() {
        dst[i] = x.unwrap_or(f64::NAN);
    }
    n as i64
}

// ── Existing core ───────────────────────────────────────────────────

// Computes the lexicographically-sorted categorical encoding of `col_name`
// inside the Rust shim — eliminates the per-cell hash-map / sort loop in
// Chuks code. Writes one i64 code per row into `codes_buf` (caller must
// provide at least `codes_cap` >= height*8 bytes) and returns the sorted
// category vocabulary as a single `sep`-joined C string. NULL codes
// encode as -1.
//
// Returns NULL on error (null df / unknown col / buffer too small / sep == 0).
//
// Caller frees the returned string via `pl_free_string`.
#[no_mangle]
pub extern "C" fn pl_col_categorical(
    df: *mut DataFrame,
    col_name: *const c_char,
    sep: u8,
    codes_buf: *mut i64,
    codes_cap: i64,
) -> *mut c_char {
    if df.is_null() || codes_buf.is_null() || sep == 0 {
        return ptr::null_mut();
    }
    let cname = match unsafe { cstr_to_str(col_name) } {
        Some(s) => s,
        None => return ptr::null_mut(),
    };
    let df_ref = unsafe { &*df };
    let series = match df_ref.column(cname) {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };
    let n = series.len();
    if (n as i64) * 8 > codes_cap {
        return ptr::null_mut();
    }

    // Materialize as Utf8 once. For non-string columns this allocates the
    // string view (e.g. integer -> "1","2"); for native string columns it
    // is a cheap clone of the underlying ChunkedArray view.
    let utf8_series = if matches!(series.dtype(), DataType::String) {
        series.as_materialized_series().clone()
    } else {
        let casted = match series.cast(&DataType::String) {
            Ok(s) => s,
            Err(_) => return ptr::null_mut(),
        };
        casted.as_materialized_series().clone()
    };
    let ca = match utf8_series.str() {
        Ok(c) => c.clone(),
        Err(_) => return ptr::null_mut(),
    };

    // Build sorted, deduplicated vocabulary. polars' `unique` does not
    // guarantee order, so we sort the result. Null values are *not*
    // included in the dictionary — they encode as -1.
    let uniq = match ca.unique() {
        Ok(u) => u,
        Err(_) => return ptr::null_mut(),
    };
    // Collect to Vec<&str> for deterministic in-process sort.
    let mut cats: Vec<&str> = uniq.into_iter().flatten().collect();
    cats.sort_unstable();

    // Dictionary: &str -> i64 code.
    use std::collections::HashMap;
    let mut dict: HashMap<&str, i64> = HashMap::with_capacity(cats.len());
    for (i, c) in cats.iter().enumerate() {
        dict.insert(*c, i as i64);
    }

    // Emit codes into the caller's buffer.
    let codes_slice: &mut [i64] = unsafe {
        std::slice::from_raw_parts_mut(codes_buf, n)
    };
    let mut idx = 0usize;
    for v in ca.into_iter() {
        codes_slice[idx] = match v {
            Some(x) => *dict.get(x).unwrap_or(&-1),
            None => -1,
        };
        idx += 1;
    }

    // Build sep-joined category string.
    let sep_ch = sep as char;
    let mut joined = String::with_capacity(cats.iter().map(|s| s.len() + 1).sum());
    for (i, c) in cats.iter().enumerate() {
        if i > 0 {
            joined.push(sep_ch);
        }
        joined.push_str(c);
    }
    ret_cstr(joined)
}

#[no_mangle]
pub extern "C" fn pl_to_string(df: *mut DataFrame) -> *mut c_char {
    if df.is_null() {
        return ptr::null_mut();
    }
    let df_ref = unsafe { &*df };
    ret_cstr(format!("{}", df_ref))
}

#[no_mangle]
pub extern "C" fn pl_free_df(df: *mut DataFrame) {
    if df.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(df);
    }
}

#[no_mangle]
pub extern "C" fn pl_free_string(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    unsafe {
        let _ = CString::from_raw(s);
    }
}

