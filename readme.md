# Search Engine

## Features
- Indexing pdf files in a directory  
- Searching of terms

## Building the binary(Linux-based-platforms)
You may need `libpoppler-glib` installed on you system.  
For arch users  

```bash
sudo pacman -S poppler-glib
```

```bash
bash build.sh
```

## Usage

Indexing 
```bash
indexer index <path_to_documents_directory> <path_to_index_file>
```

Searching
```bash
indexer search <path_to_index.json> <query>
```

## Licensing
The project is licensed under the [GPL3 License](LICENSE)
