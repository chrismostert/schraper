use super::Runnable;
use anyhow::Result;
use sqlx::PgPool;

#[derive(Debug)]
pub struct BeerFetcher {
    pub pool: PgPool,
}
impl Runnable for BeerFetcher {
    async fn run(&self) -> Result<()> {
        println!("Ran the fetcher for beers");
        Ok(())
    }
}
