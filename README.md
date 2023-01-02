# Fork of Actix Web
[origin README.md](actix-web/README.md)
## Purpose of fork
This fork is currently focus on adding support for serving pre-compressed gzip/brotli files from disk in actix-files.

## Usage
use Files
```rust
use actix_files::Files;
use actix_web::http::header::ContentEncoding;

let files_service = Files::new("/", "./static")
    .use_precompressed(vec![ContentEncoding::Brotli, ContentEncoding::Gzip]);
```

use NamedFile
```rust
use actix_files::NamedFile;
use actix_web::http::header::ContentEncoding;

# async fn open() {
// file1 should be exactly the same as file2 if foo.txt.br exist.
let file1 = NamedFile::open_compressed("foo.txt", encodings: &vec![ContentEncoding::Brotli, ContentEncoding::Gzip]).await.unwrap();
let file2 = NamedFile::open_async("foo.txt.br").await.unwrap()e.set_content_encoding(ContentEncoding::Brotli);
# }
```

## License
The modified / added code is license to public domain. Code from original repository follow their own licence. see [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).

## Declarations
- THE MODIFIED / ADDED CODE IS PROVIDED "AS IS", WITHOUT ANY WARRANTY.
- The fork owner MAY or MAY NOT make any pull request to original repository.
- The fork owner will NOT publish any crate of the fork.
- The fork owner MAY NOT wirte test for modified / added code, and MAY NOT run CI actions.
