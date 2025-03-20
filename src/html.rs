/// The default HTML template for a simple web page for serving and making requests from the
/// search engine's on the backend
pub const HTML_DEFAULT: &str = r"
<!doctype html>
<html>
  <head>
    <title>Indexer</title>
    <meta charset='utf-8' />
  </head>
  <body>
    <h1>Type a query to search</h1>
    <input type='text' id='query' value='' />
    <ul id='results'></ul>

    <script>
      document.getElementById('query').addEventListener('change', (e) => {
        fetch('/query', {
          method: 'POST',
          headers: {
            'Content-Type': 'text/plain',
          },
          body: e.currentTarget.value,
        })
          .then((response) => response.text())
          .then((result) => {
            // result is a string of strings separated by newline
            const list_items = result.split('\n');
            let results = document.getElementById('results');

            // Clear previous results
            results.innerHTML = '';

            list_items.forEach((item) => {
              if (item.trim() !== '') {
                const li = document.createElement('li');
                li.textContent = item;
                results.appendChild(li);
              }
            });
          })
          .catch((err) => console.error(err));

      });
    </script>
  </body>
</html>
";
