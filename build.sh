echo "Building project..."

cargo build --release

sudo cp target/release/indexer /usr/local/bin/

mkdir "$HOME/.indexer" 
cp index.html "$HOME/.indexer/"

echo "To use the tool run: indexer"
