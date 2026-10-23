# $\ket{db}$

Thread-per-core distributed HTAP database powered by io-uring.

> [!WARNING]
> This project is work in progress :construction:

## Features

- Run WebAssembly or Python on the database node for custom data processing.
- Tiered storage with compression and encryption.

## Concepts

### SIMD

SIMD is needed to support HTAP workloads. This allows for vectorized processing of data in memory. So values are grouped together by column instead of by row.

### O_DIRECT

To prevent double caching of data in the kernel and in our own buffer cache, we use O_DIRECT to only have our own cache. This means the buffers must be page aligned.

### Key Management Systems (KMS)

Typically, we don't want to encrypt all the data using a single key and we don't want to permanently store the keys on disk. Instead, we authenticate with a KMS and have it provide us with a key to encrypt and decrypt the data. On key creation, it returns us a wrapped key (encrypted key) and the plain text key. We store the wrapped key on disk and zeroize the plain text key after we are done using it. When it is time to decrypt, we tell the KMS to unwrap the key to reveal the plain text key that we then use to decrypt.

## Packages

The packages are roughly divided in a way to prevent circular dependencies.

### Storage

Contains the main storage utilities to be used by the storage engine:
- `Storage` owns a `FrameManager`, a `KeyManager`, and a `FileManager`. It is the interface for reading and writing frames and for inserting and expiring keys.
- `Frame` are the cooridnates to a buffer in a file. Where is the buffer? How is it encoded?
- `FrameManager` caches buffers after decoding them from read operations. It also supports writing. Methods that touch files or keys take a `&mut FileManager` and a `&mut KeyManager`.
- `FileManager` caches open files by `FileId`.
- `Segment` contains part of the rows in the table. All rows within it are sorted by the first column (column 0).
- `SegmentMut` is used to create new segments and write them to disk.
- `Column` contains all the blocks for a column in a segment.
- `Block` contains the underlying values for a column in a segment.
- `BlockMut` is the mutable version of the block. This is used inside `SegmentMut`.
- `Buffer` is the page aligned buffer that is used to read and write data to disk.
- `KeyManager` is a cache that maps `FileId` to the plain text key.
- `SegmentFooter` contains the handles to properly access the blocks within a segment.
- `DeleteMask` is a bitmap that indicates which rows in a segment are logically deleted since segments are immutable.
- `Batch` is a batch of values returned when reading a block.
- Min-`Block` and Max-`Block` are special `Block` units used to store the range of values per block in a column. It can also be used by the cursor to do binary-search to find the block that contains the value.
- `BloomFilter` is used on the segment to quickly check if a value is not present in a column within a segment, so we can skip them.

### Engine

Contains the storage engine:
- `BlockCursor` is used to seek and read values from a block.
- `SegmentCursor` combines the `BlockCursors` of the requested columns.
- `OrderedTableCursor` combines the `SegmentCursors` of a table with merge-sort to return the rows ordered by the first column. This is useful to retrieve few rows that are properly index.
- `TableCursor` combines the `SegmentCursors` of a table without any order guarantees. This is useful for analytical queries that need to scan the entire table.
- `Compactor` is used to merge segments that are logically smaller than a certain threshold into a single segment. This will use the `OrderedTableCursor`.
- `MemTable` is the row-oriented in-memory table that aggregates the writes before flushing them to disk. This adds support for transactional writes and reads.
- `Wal` is the write-ahead log to store changes that are not flushed to disk yet.
- `Table` is the main transactional interface to read and write data to a table.

### Executor

Contains the query execution utilities to be used by the database.
