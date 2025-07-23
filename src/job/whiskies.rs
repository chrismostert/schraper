use super::Runnable;
use anyhow::Result;
use sqlx::PgPool;

#[derive(Debug)]
pub struct WhiskeyFetcher {
    pub pool: PgPool,
}
impl Runnable for WhiskeyFetcher {
    async fn run(&self) -> Result<()> {
        println!("Ran the fetcher for whiskies");
        Ok(())
    }
}
