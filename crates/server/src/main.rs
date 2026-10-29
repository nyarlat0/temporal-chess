#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    temporal_chess_server::run().await
}
