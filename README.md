# chuks_polars — User Manual

Column-oriented analytics for Chuks, powered by the [polars](https://pola.rs)
Rust crate. Read CSV / Parquet, select, filter, group, join, aggregate, compute
column expressions, render tables, and plot, all at native speed.

---

## Install

```bash
chuks add @chuks/polars
```

---

## Quick start

```chuks
import { Polars, DataFrame } from "pkg/@chuks/polars"

const pl = new Polars()                       // builds/loads the cached shim
const df = pl.readCsv("./data/airbnb.csv")

println("shape: " + string(df.height()) + " x " + string(df.width()))
println("cols:  " + df.columns().join(", "))

const top = df.filter("price", ">", "100")
              .groupBy("neighbourhood_group", "price:mean,id:count")
              .sortBy("price_mean", "1")
println(top.toString())

top.close()
df.close()
```

> **Memory.** Every DataFrame returned by a Polars/DataFrame method owns
> Rust-side heap state. Call `.close()` when done (safe to call multiple times).
> Inside the [Jupyter kernel](https://chuks.org/ai-data/jupyter) each cell is its
> own process, so closing is cosmetic — the process exits and frees everything.

---

## Loading data

`new Polars(shimDir?)` builds (or loads the cached) Rust shim and returns the
engine handle. `shimDir` defaults to `chuks_packages/@chuks/polars`; for an
installed package the default normally resolves. If you vendored the package
elsewhere, pass the path to the shim crate explicitly, e.g.
`new Polars("./chuks_packages/@chuks/polars/shim")`.

| Constructor / method                                    | Description                                                                                   |
| ------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `new Polars(shimDir?)`                                  | Build/load the shim. Returns the engine handle.                                               |
| `pl.readCsv(path)`                                      | Read a CSV with defaults (comma separator, has header).                                       |
| `pl.readCsvOpts(path, sep, hasHeader, skipRows, nRows)` | Custom CSV — `sep` is a byte (e.g. `';'.charCodeAt(0)`), `hasHeader` bool, `nRows < 0` = all. |
| `pl.readParquet(path)`                                  | Read a Parquet file (with statistics-based predicate pushdown).                               |

```chuks
const tsv  = pl.readCsvOpts("./data/dump.tsv", 9, true, 0, -1)  // 9 = tab byte
const parq = pl.readParquet("./data/orders.parquet")
```

---

## Inspection

| Method             | Returns    | Description                                      |
| ------------------ | ---------- | ------------------------------------------------ |
| `df.height()`      | `int`      | Row count.                                       |
| `df.width()`       | `int`      | Column count.                                    |
| `df.shape()`       | `[]int`    | `[height, width]`.                               |
| `df.columns()`     | `[]string` | Column names in order.                           |
| `df.dtypes()`      | `[]string` | Per-column Polars dtype names.                   |
| `df.nullCounts()`  | `[]int`    | Null count per column, aligned with `columns()`. |
| `df.get(col, row)` | `string`   | Single cell as a string (empty for null).        |

---

## Selection

| Method                             | Returns     | Description                                                       |
| ---------------------------------- | ----------- | ----------------------------------------------------------------- |
| `df.head(n)` / `df.tail(n)`        | `DataFrame` | First / last `n` rows.                                            |
| `df.slice(offset, length)`         | `DataFrame` | Row slice.                                                        |
| `df.sampleN(n, seed)`              | `DataFrame` | Random sample; `seed < 0` for non-deterministic.                  |
| `df.unique(subsetCsv, keep)`       | `DataFrame` | Drop duplicates. `keep` ∈ `"first"`, `"last"`, `"any"`, `"none"`. |
| `df.select("a,b,c")`               | `DataFrame` | Keep only these columns.                                          |
| `df.drop("a,b")`                   | `DataFrame` | Drop these columns.                                               |
| `df.rename("old1=new1,old2=new2")` | `DataFrame` | Rename columns.                                                   |

---

## Sort & filter

```chuks
// Sort by multiple columns. descCsv = "1,0" means desc, then asc.
const sorted = df.sortBy("price,reviews", "1,0")

// Single-column filter. ops: eq/== ne/!= gt/> lt/< gte/>= lte/<=
const expensive = df.filter("price", ">", "200")

// Null helpers
const clean = df.dropNullsIn("price,reviews")
```

| Method                                           | Description                                               |
| ------------------------------------------------ | --------------------------------------------------------- |
| `df.sortBy(colsCsv, descCsv)`                    | Sort by multiple columns; `descCsv` is per-column 1/0.    |
| `df.filter(col, op, value)`                      | Filter rows where `col` compared to `value` matches `op`. |
| `df.filterIsNull(col)` / `df.filterNotNull(col)` | Keep rows where the column is null / not-null.            |
| `df.dropNulls()` / `df.dropNullsIn(colsCsv)`     | Drop any row with a null in _any_ / specified columns.    |

---

## GroupBy & aggregations

```chuks
// One groupBy can compute many aggregations at once.
const agg = df.groupBy(
    "neighbourhood_group",                 // group keys (CSV)
    "price:mean,price:median,id:count"     // aggregations (CSV)
)
// Output columns: neighbourhood_group, price_mean, price_median, id_count
```

**Supported aggregation ops** (right side of `col:op`):
`sum`, `mean` (alias `avg`), `min`, `max`, `count`, `std`, `var`, `median`,
`first`, `last`, `n_unique` (alias `count_distinct`).

There is also a single-aggregation convenience:

```chuks
const totals = df.groupbyAgg("name", "amount")   // sum of `amount` per `name`
```

---

## Joins & concatenation

```chuks
const customers = pl.readCsv("./data/customers.csv")
const orders    = pl.readCsv("./data/orders.csv")

const joined = customers.join(orders, "id", "customer_id", "inner")
//                              left key, right key, how
```

`how` ∈ `"inner"`, `"left"`, `"outer"` (or `"full"`), `"cross"`, `"semi"`,
`"anti"`. The key arguments accept CSV for multi-column joins.

`df.vstack(other)` stacks rows vertically — schemas must match.

---

## Column scalars

Fast Rust-side reductions on a single numeric column:

| Method                              | Returns | Description      |
| ----------------------------------- | ------- | ---------------- |
| `df.colSum(col)`                    | `float` | Sum.             |
| `df.colMean(col)`                   | `float` | Arithmetic mean. |
| `df.colMin(col)` / `df.colMax(col)` | `float` | Extremes.        |
| `df.colCount(col)`                  | `int`   | Non-null count.  |

```chuks
const total: float = df.colSum("amount")
const avg:   float = df.colMean("price")
```

---

## Extracting columns

| Method                | Returns        | Description                                                      |
| --------------------- | -------------- | ---------------------------------------------------------------- |
| `df.column(col)`      | `[]string`     | A whole column as strings (empty string for null).               |
| `df.series(col)`      | `Series`       | A fluent column expression (see [Series](#series)).              |
| `df.matrix(cols)`     | `PolarsMatrix` | Numeric columns as a matrix (see [PolarsMatrix](#polarsmatrix)). |
| `df.categorical(col)` | `[]any`        | Distinct category codes for a column (handy for encoding).       |

---

## Series

`df.series(col)` returns a **Series** — a lazily-built, chainable column
expression. Comparison/arithmetic ops take a `float` scalar on the right;
logical ops take another `Series`. Nothing is computed until you materialize
with `.toFloats()`, `.toInts()`, or `.toStrings()`.

```chuks
// Boolean mask as a 0/1 float column.
const yF: []float = df.series("target").eq(2.0).toFloats()

// Compose freely: z-score normalize a column.
const mean: float = df.colMean("amount")
const std:  float = 1.0   // compute your own std, or pull from groupBy
const norm: []float = df.series("amount")
    .fillNull(0.0)
    .sub(mean).div(std)
    .toFloats()
```

| Category    | Methods                                                                    | Right-hand     |
| ----------- | -------------------------------------------------------------------------- | -------------- |
| Info        | `name()` → `string`, `length()` → `int`                                    | —              |
| Compare     | `eq` `ne` `gt` `lt` `ge` `le`                                              | `float` scalar |
| Arithmetic  | `add` `sub` `mul` `div`                                                    | `float` scalar |
| Logical     | `and_` `or_` (other: `Series`), `not_()`                                   | `Series`       |
| Cast        | `cast("f64" \| "i64" \| "bool")`                                           | —              |
| Null        | `fillNull(value)`, `isNull()`, `isNotNull()`                               | —              |
| Materialize | `toFloats()` → `[]float`, `toInts()` → `[]int`, `toStrings()` → `[]string` | —              |

> **Don't iterate a Series row-by-row.** It is decorated with `@iterationCost`,
> so `for (var v of series)` raises a typechecker warning — row-by-row FFI is
> ~200× slower than columnar materialization. Materialize once
> (`series.toFloats()`), then iterate the plain slice.

---

## PolarsMatrix

`df.matrix(cols)` returns a **PolarsMatrix** (a `Matrix` subclass) — numeric
columns packed for linear-algebra / ML feature work.

| Method         | Returns     | Description                          |
| -------------- | ----------- | ------------------------------------ |
| `m.columns()`  | `[]string`  | The column names backing the matrix. |
| `m.toFloats()` | `[]float`   | Flat row-major values.               |
| `m.toRows()`   | `[][]float` | Values as a list of rows.            |

```chuks
const X = df.select("f1,f2,f3").matrix(["f1", "f2", "f3"])
const rows: [][]float = X.toRows()
```

---

## I/O

| Method                  | Description                             |
| ----------------------- | --------------------------------------- |
| `df.writeCsv(path)`     | Write CSV (header included).            |
| `df.writeParquet(path)` | Write Parquet with default compression. |

---

## Rendering

| Method                   | Returns / does                            | Use case                                                      |
| ------------------------ | ----------------------------------------- | ------------------------------------------------------------- |
| `df.toString()`          | `string` — Polars ASCII table.            | Logging / terminals.                                          |
| `df.show(n)`             | Prints up to `n` rows (`n <= 0` = all).   | Terminal scripts.                                             |
| `df.display(maxRows)`    | Emits a Jupyter MIME bundle (HTML table). | Inside the [Chuks kernel](https://chuks.org/ai-data/jupyter). |
| `df.toHtml(maxRows)`     | `string` — bare `<table>` HTML.           | Embed in your own page.                                       |
| `df.toMarkdown(maxRows)` | `string` — GitHub-flavored MD table.      | Reports, PR comments.                                         |

```chuks
df.head(10).display(0)        // → interactive HTML table in the notebook
println(df.toMarkdown(20))    // → MD table to stdout
```

---

## Inline plotting (Chart.js, self-contained HTML)

`chuks_polars` ships standalone plot helpers that write a self-contained
Chart.js HTML page to disk — useful when you don't want a separate chart
library. For inline notebook charts, prefer
[`chuks_viz`](https://chuks.org/ai-data/viz) — it emits Vega-Lite specs that
render directly in the cell.

| Method                                           | Output                |
| ------------------------------------------------ | --------------------- |
| `df.plotBar(labelCol, valueCol, title, outPath)` | Bar chart HTML file.  |
| `df.plotLine(xCol, yCol, title, outPath)`        | Line chart HTML file. |
| `df.plotPie(labelCol, valueCol, title, outPath)` | Pie chart HTML file.  |
| `df.plotHistogram(col, bins, title, outPath)`    | Histogram HTML file.  |

Each returns the output path as a `string`.

---

## Full worked example

```chuks
import { Polars, DataFrame } from "pkg/@chuks/polars"

const pl = new Polars()
const df = pl.readCsv("./data/airbnb.csv")

// 1. Profile
println("shape: " + string(df.height()) + " x " + string(df.width()))
const cols  = df.columns()
const types = df.dtypes()
const nulls = df.nullCounts()
var i: int = 0
while (i < cols.length) {
    println("  " + cols[i] + ": " + types[i] + " (" + string(nulls[i]) + " nulls)")
    i = i + 1
}

// 2. Clean
const clean = df.dropNullsIn("price,reviews_per_month")
                .filter("price", "<", "1000")     // strip outliers

// 3. Aggregate
const byBorough = clean
    .groupBy("neighbourhood_group", "price:mean,reviews_per_month:mean,id:count")
    .sortBy("price_mean", "1")
println(byBorough.toString())

// 4. Export
byBorough.writeCsv("./out/by_borough.csv")

// 5. Tidy up
byBorough.close()
clean.close()
df.close()
```

---

## Performance notes

- **First-use compile.** The Rust shim builds polars + dependencies on the first
  `new Polars(...)` call — 1–3 minutes once, then cached forever at
  `~/.chuks/cache/native/rc_<sha>.<ext>`.
- **FFI cost.** Every method crosses a C ABI. For tight inner loops (millions of
  cell-by-cell reads), prefer column-vectorized operations (`groupBy`, `filter`,
  `colSum`, `series(...).toFloats()`) over `for ri … get(col, ri)`.
- **Memory.** DataFrames are Rust-owned; call `.close()` to free. Inside the
  kernel each cell is a fresh process, so leaks are bounded to one cell.

---

## Limitations

- The CSV reader auto-detects column types — use `readCsvOpts` for fine control.
- Only the operations documented here are exposed. The shim is intentionally
  lean; if you need a polars op that isn't here,
  [open an issue](https://github.com/chukspackages/chuks_polars/issues) — adding
  a method is usually under 30 lines (one FFI signature + one Chuks wrapper).
- No lazy-frame API yet — `chuks_polars` calls eager `DataFrame` methods. For
  very large data, push aggregations down with `select` + `filter` before
  `groupBy`.

---

## See also

- [chuks_viz — Interactive charts](https://chuks.org/ai-data/viz)
- [Jupyter Notebooks in VS Code](https://chuks.org/ai-data/jupyter)
- [Apache Arrow CDI](https://chuks.org/stdlib/chuks-arrow) — zero-copy exchange with DuckDB, pyarrow, etc.
- [chuksToRust FFI](https://chuks.org/stdlib/chuks-to-rust) — build your own Rust bindings.
