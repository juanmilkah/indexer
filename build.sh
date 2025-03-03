echo "Building project..."

cargo build --release

sudo cp target/release/indexer /usr/local/bin/

echo "To use the tool run: indexer"
