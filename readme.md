# Search Engine

## Features
- Indexing pdf files in a directory  
- Searching of terms

## Building the binary(Linux-based-platforms)
```bash
bash build.sh
```

## Usage

Indexing 
```bash
indexer index <path_to_document_directory>
```

Searching
```bash
indexer search <path_to_index.json> "foo bar baz"
```

## Licensing
The project is licensed under the [GPL3 License](LICENSE)
