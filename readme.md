# Vector-Space Search Engine

## Features
- Indexing pdf files in a directory  
- Searching of terms

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
If the path to index file is not specified the fallback is `index.json`.  
Supported file types:  
(pdf, txt, md, xml, xhtml)

```bash
indexer index [path_to_documents_directory] [path_to_index_file]
```


- ### Searching
```bash
indexer query <path_to_index.json> <query>
```

- ### Serving via http server
Localhost on port `8080`
```bash
indexer serve <path_to_index_file>
```

```bash
http POST :8080/search query="foo bar baz"
```

- ### Help page
```bash
indexer --help
```

- ### Version Info
```bash
indexer --version
```

## Licensing
The project is licensed under the [GPL3 License](LICENSE)
