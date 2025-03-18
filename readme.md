# Local Search Engine

![Flamegraph](flamegraph.svg)

A search engine for local directories implemented in Rust.  
It employs the [tf-idf](https://en.wikipedia.org/wiki/Tf%E2%80%93idf) algorithm for file indexing, [snowball](https://snowballstem.org/) stemming algorithms for token stemming.

## Features

- Indexing pdf files in a directory
- Querying of terms
- Serve via http

## Installation

You may need `libpoppler-glib` installed on you system.  
For arch users

```bash
sudo pacman -S poppler-glib
```

```bash
git clone https://github.com/juanmilkah/indexer
cd indexer
bash build.sh
```

## Usage

- ### Indexing
  If path to docs is not provided it falls back to the current directory.  
  Supported file types:  
  (pdf, txt, md, xml, xhtml, html, csv)

```bash
indexer index -p <path_to_document> -o <path_to_output_file>
```

You can also redirect Stderr to a file via the `log` argument.

```bash
indexer -l indexer.log index -p <~/documents> -o <output_file>
```

- ### Querying

```bash
indexer query -i <path_to_index_file> -q <query> -o [output_file]
```

- ### Serving via http server
  Localhost on port `8080`
  The average latency for a query is `45ms`

```bash
indexer serve -i <path_to_index_file> -p [port]
```

```bash
curl -X POST http://localhost:8080/query d "foo bar baz"
```

- ### Help page

```bash
indexer --help
```

- ### Version Info

```bash
indexer --version
```

### TODO

Additional optimizations.

Memory-mapped files: For very large datasets, consider using memory-mapped files (mmap) for faster I/O.
Streaming parser: Implement streaming parsers for large documents to avoid loading entire files into memory.
Compression: Use fast compression algorithms like LZ4 or Zstd for the index to reduce I/O overhead.
Incremental updates: Implement a more efficient incremental update mechanism instead of full re-indexing.
Async I/O: Consider using async I/O with Tokio for file operations to avoid blocking threads.

## Licensing

The project is licensed under the [GPL3 License](LICENSE)
