# Indexer - A Minimalistic Search Engine

A fast, lightweight search engine written in Rust that can index and search
through various document formats using TF-IDF scoring.

## Features

- **Multiple Format Support**: CSV, HTML, PDF, XML, TXT, Markdown
- **Stemming**: English Porter2 stemming algorithm
- **Stop Words**: Automatic filtering of common English stop words
- **Parallel Processing**: Multi-threaded indexing for performance
- **Web Interface**: HTTP server with search API
- **Incremental Updates**: Skip unchanged files during re-indexing
- **TF-IDF Scoring**: Relevance-based search results

## Installation

```bash
git clone https://github.com/juanmilkah/indexer
cd indexer
bash build.sh
```

## Usage

### Building an Index

Index all files in the current directory:
```bash
indexer index
```

Index a specific directory:
```bash
indexer index --path /path/to/documents
```

Index with custom output directory:
```bash
indexer index --path ./docs --output ./my_index
```

Include hidden files and directories:
```bash
indexer index --path ./docs --hidden
```

Skip specific directories or files:
```bash
indexer index --path ./project --skip-paths target node_modules .git
```

### Searching

Search the default index:
```bash
indexer search --query "machine learning"
```

Search with specific index directory:
```bash
indexer search --index ./my_index --query "rust programming"
```

Limit number of results:
```bash
indexer search --query "database" --count 10
```

Save results to file:
```bash
indexer search --query "algorithm" --output results.txt
```

### Web Server

Start the web server on default port (8765):
```bash
indexer serve
```

Start on custom port with specific index:
```bash
indexer serve --index ./my_index --port 3000
```

The web interface will be available at `http://localhost:8765`

## Architecture

### Core Components

#### MainIndex (`tree.rs`)
The main index manages the inverted index structure:
- **DocumentStore**: Maps file paths to document IDs
- **InMemorySegment**: Temporary storage before flushing to disk
- **Segments**: Persistent storage units containing term dictionaries and 
  postings lists

#### Lexer (`lexer.rs`)
Tokenizes text content:
- Handles numeric, alphabetic, and special characters
- Applies English stemming using Porter2 algorithm
- Filters stop words

#### Parsers (`parsers.rs`)
Document-specific parsers for different file formats:
- **CSV**: Extracts text from all fields
- **HTML**: Parses and extracts visible text content
- **PDF**: Extracts text from all pages
- **XML**: Extracts character data from elements
- **Text/Markdown**: Direct text processing

#### Server (`server.rs`)
HTTP server providing search functionality:
- `GET /`: Serves HTML search interface
- `POST /query`: Processes search queries and returns results

### Data Flow

1. **Indexing**: Files → Parser → Lexer → Tokens → InMemorySegment → 
   Disk Segments
2. **Searching**: Query → Lexer → Tokens → Segment Lookup → TF-IDF 
   Calculation → Ranked Results

### File Structure

```
~/.indexer/                    # Default index directory
├── docstore.bin               # Document metadata
├── segment_0/                 # First segment
│   ├── term.dict              # Term dictionary
│   └── postings.bin           # Postings lists
├── segment_1/                 # Additional segments...
│   ├── term.dict
│   └── postings.bin
└── logs                       # Application logs
```

## Configuration

### Environment

The indexer uses `~/.indexer` as the default storage directory. This can be
overridden using the `--output` flag for indexing or `--index` flag for
searching.

### Supported File Extensions

- **Text**: `.txt`, `.md`
- **Web**: `.html`, `.xml`, `.xhtml`
- **Data**: `.csv`
- **Documents**: `.pdf`

### Performance Tuning

- **Segment Size**: Default 100 documents per segment (configurable in code)
- **Parallel Processing**: Uses all available CPU cores for indexing
- **Memory Usage**: Segments are flushed to disk when full

## Command Reference

### Global Options

- `-l, --log <FILE>`: Redirect logs to specific file

### Index Command

```bash
indexer index [OPTIONS]
```

**Options:**
- `-p, --path <PATH>`: Directory or file to index
- `-o, --output <DIR>`: Index output directory
- `-z, --hidden`: Include hidden files and directories
- `-s, --skip-paths <PATHS>`: Skip specific paths (space-separated)

### Search Command

```bash
indexer search [OPTIONS] --query <QUERY>
```

**Options:**
- `-i, --index <DIR>`: Index directory to search
- `-q, --query <QUERY>`: Search terms
- `-o, --output <FILE>`: Save results to file
- `-c, --count <NUMBER>`: Maximum number of results

### Serve Command

```bash
indexer serve [OPTIONS]
```

**Options:**
- `-i, --index <DIR>`: Index directory to serve
- `-p, --port <PORT>`: Port number (default: 8765)

## API Reference

### HTTP Endpoints

#### GET /
Returns the HTML search interface.

#### POST /query
Accepts search query in request body and returns matching documents.

**Response Format:**
```
/path/to/document1.txt
/path/to/document2.pdf
/path/to/document3.html
```

## Technical Details

### TF-IDF Implementation

The search engine uses Term Frequency-Inverse Document Frequency scoring:

- **TF (Term Frequency)**: Number of times a term appears in a document
- **IDF (Inverse Document Frequency)**: `ln(total_docs / docs_containing_term)`
- **Score**: `TF × IDF` summed across all query terms

### Stemming

Uses the `rust-stemmers` crate with the English Porter2 algorithm to reduce
words to their root forms (e.g., "running" → "run").

### Stop Words

Common English words (the, and, or, etc.) are filtered out during indexing
and searching using the `stop-words` crate.

### Serialization

- **Document Store**: Binary serialization using `bincode2`
- **Postings Lists**: Binary serialization for efficient storage and retrieval
- **Term Dictionaries**: HashMap serialization for fast term lookups

## Troubleshooting

### Common Issues

**Permission denied**:
```bash
# Check file permissions or run with appropriate privileges
chmod +r /path/to/documents/*
```

### Log Files

Application logs are stored in `~/.indexer/logs` by default. Use the `--log`
flag to specify a different location.

## Development

### Building from Source

```bash
git clone <repository>
cd indexer
bash build.sh
```

### Contributing

1. Fork the repository
2. Create a feature branch
3. Make changes with appropriate tests
4. Submit a pull request

## License

[GPL3](LICENSE)

## Version

Check version with:
```bash
indexer --version
```
