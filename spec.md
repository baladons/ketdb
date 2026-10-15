# $\ket{db}$

KetDB is a Rust wide-column store that leverages zero-copy `io-uring` along with `simd` with thread-per-core architecture in mind.

## Store Module

This module will focus only on the storage format, caching layer and file IO of this database.

### Table Footer

Please see `footer.rs`.

Essentially, data blocks store the actual data for a given column. To speedup lookups, we also store the min and max values of a given block as an entry inside the min and max blocks for quick filtering based on range. This also allows us to do binary-search on the min blocks if the data is sorted.

For example, assume we are in column 0 with the following two data blocks:

```
data block 0: [1, 2, 3, 4, 5]
data block 1: [6, 7, 8, 9, 10]
```

Then the min and max blocks will look like:

```
min block: [1, 6]
max block: [5, 10]
```

In addition, to speedup point lookups, we also store a bloom filter for the entire column. So there should only be one bloom block per column.

### Data Blocks

There are two types of data blocks:

- fixed-width are used to store data blocks that contain data of the same size.
- variable-width are used to store data blocks that contain data of different sizes.

See `block_reader.rs` and `block_writer.rs` for more details.

### Block handles

Each on-disk block has a `BlockHandle`.

- `start_offset` is the page-aligned file offset where the block was written.
- `end_offset` is the encoded byte length of that block, without the 4096-byte padding written to disk.
- For a data block, `start_row_index` is the first table row in the block.
- For a min or max block, `start_row_index` is the first data block index covered by that block. Entry `i` describes data block `start_row_index + i`.

### Min and Max Blocks

These are data blocks of per-data-block min and max values. Binary search on the min blocks finds the data block for a key: `start_row_index + index_within_min_block`.

### Bloom Blocks

These are Bloom filters packaged as a block. They do not follow the data block format. There is one bloom block per column, sized from `ColumnConfig.bloom_expected_items` and hashed with `BLOOM_SEED`. `has_value(column_id, value)` on a table cursor checks that column's filter. A merged cursor checks each table and returns true if any column filter may contain the value.

## File IO

All file IO uses `O_DIRECT`. Reads and writes go through `AlignedVec`, a `Vec` of 4096-byte pages aligned to 4096 bytes and compatible with compio `IoBuf`. The logical length is the real payload. The I/O length is that size rounded up to a page, with the tail zeroed.

The file ends with the rkyv footer, then a 16-byte trailer (`footer_offset`, `footer_len`) at the start of the last page. `footer_len` is the unpadded footer size.

### Memtable

Writes land in a `MemTable` and are appended to its WAL before the in-memory row is updated. `WriteConcern.buffered_journal` keeps those records in memory until the 4096-byte journal page is full. When it is false, every write flushes that page. At `MEMTABLE_MAX_ENTRIES` (100k) writes, the rows are flushed in key order into a new table.

`data/metadata.ket` is the single database metadata file. The decoded document is cached on the thread `Context`. Callers borrow that value and write it back in place. It stores an rkyv `Metadata` document whose `groups` list is one `TableGroup` per table. Flushed tables are a `Vec<Uuid>` stored newest first. Compaction edits that vector directly. The group also stores its column ids, the WAL id backing its memtable, and `deleted_rows`, the number of rows marked deleted.

`Store` opens every group. `create_table` / `delete_table` add and remove a group. `flush` writes that group's memtable into a table file. `insert`, `update`, `delete`, and `read` take a table group id and column ids. `read` returns a `ReadRow`; `get` borrows the memtable cell or the cached data block. Memtable point reads use the B-tree. Column 0 is the row key and is stored first. Insert writes the given columns and keeps the others. Update patches an existing row. A row already in the memtable is patched in place; cells are copied only when the row is created or loaded from a flushed table. Delete clears the named columns, and naming the key column removes the row.

Each memtable entry is `Option`: `Some` inserts or replaces a row, `None` deletes it. On flush, a delete or replace resolves the key to a row index in the newest live table and sets that bit in `data/{table_id}.mask.ket`. The mask is a raw bit vector, padded to 4096 bytes, with no compression or encryption. Those 4096-byte pages are cached on the thread `Context`. A point read tests its bit on the cached page. New rows are written to a new table. After every flush, adjacent tables whose live rows (row count minus deleted bits) sum to `MEMTABLE_MAX_ENTRIES` or fewer are merged into one table. Deleted rows are dropped, the new table gets an empty mask, and `deleted_rows` decreases by the bits that were removed. `MergedTableCursor::from_memtable` merges the B-tree with every flushed table in key order and skips masked rows and memtable tombstones. The memtable wins when a key is in more than one place.

