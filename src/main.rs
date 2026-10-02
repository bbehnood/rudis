use std::io;

use rudis::Server;

#[tokio::main]
async fn main() -> io::Result<()> {
    let server = Server::bind("127.0.0.1:6379").await?;

    println!("rudis listening on 127.0.0.1:6379");

    server.run().await?;

    Ok(())
}
