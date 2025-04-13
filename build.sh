echo "Building project..."

cargo build --release

sudo cp target/release/indexer /usr/local/bin/

mkdir -p $HOME/.indexer

touch $HOME/.indexer/indexfile
  
echo "To use the tool run: indexer"
