echo "Building project..."

cargo build --release

sudo cp target/release/indexer /usr/local/bin/

mkdir -p $HOME/.indexer

echo "To use the tool run: indexer"
