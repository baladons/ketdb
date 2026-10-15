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

### Min and Max Blocks

These are essentially data blocks that store the min and max values.

### Bloom Blocks

These are just Bloom filters packaged as a block. They do not follow the above data block format.

## File IO

`O_DIRECT` should be used for all file IOs. This means the file offsets need to be a multiple of 4096 bytes. The buffers' size and offset also needs to be a multiple of 4096 bytes. (TODO)

