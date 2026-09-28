use tokio::{io::AsyncWriteExt, net::TcpListener};

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:6379").await.unwrap();

    println!("Listening on 127.0.0.1:8000");

    loop {
        let (mut stream, addr) = listener.accept().await.unwrap();

        println!("New Connection from {addr}");

        tokio::spawn(async move {
            stream.write_all(b"OK").await.unwrap();
        });
    }
}
