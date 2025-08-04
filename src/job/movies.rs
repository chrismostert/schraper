use std::collections::{HashMap, HashSet};

use crate::job::matching::best_rt_hit;
use crate::job::util::JsonDecodeError;

use super::{Runnable, util::Client};
use anyhow::{Context, Result, bail};
use chrono::{Datelike, NaiveDate};
use serde::Deserialize;
use sqlx::PgPool;
use tokio::try_join;

use sqlx_batch::BatchInserter;

static PATHE_DATE_FORMAT: &str = "%Y-%m-%d";

#[derive(Deserialize, Debug, BatchInserter)]
#[serde(rename_all = "camelCase")]
#[pgtable = "cinemas"]
struct Cinema {
    #[key]
    slug: String,
    city_slug: String,
    name: String,
}

#[derive(Deserialize, Debug, BatchInserter)]
#[pgtable = "cities"]
struct City {
    #[key]
    slug: String,
    name: String,
}

#[derive(Deserialize, Debug)]
struct Shows {
    shows: Vec<Show>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Show {
    slug: String,
    title: String,
    release_at: Vec<String>,
    poster_path: Option<Image>,
    #[serde(rename = "type")]
    movie_type: String,
    duration: i32,
    genres: Vec<String>,
}

impl Show {
    fn flatten(self) -> (FlatShow, Poster, Vec<Genre>) {
        (
            FlatShow {
                slug: self.slug.clone(),
                title: self.title,
                release_at: self.release_at.into_iter().next().map(|date_str| {
                    NaiveDate::parse_from_str(&date_str, PATHE_DATE_FORMAT).unwrap()
                }),
                duration: self.duration,
                movie_type: self.movie_type,
            },
            Poster {
                show_slug: self.slug.clone(),
                lg: self.poster_path.clone().map(|p| p.lg),
                md: self.poster_path.map(|p| p.md),
            },
            self.genres.into_iter().fold(Vec::new(), |mut acc, elem| {
                acc.push(Genre {
                    show_slug: self.slug.clone(),
                    genre: elem,
                });
                acc
            }),
        )
    }
}

#[derive(Debug, BatchInserter)]
#[pgtable = "shows"]
struct FlatShow {
    #[key]
    slug: String,
    title: String,
    release_at: Option<NaiveDate>,
    movie_type: String,
    duration: i32,
}

#[derive(Debug, BatchInserter)]
#[pgtable = "posters"]
struct Poster {
    #[key]
    show_slug: String,
    lg: Option<String>,
    md: Option<String>,
}

#[derive(Debug, BatchInserter)]
#[pgtable = "genres"]
struct Genre {
    #[key]
    show_slug: String,
    #[key]
    genre: String,
}

#[derive(Deserialize, Debug)]
struct CinemaShows {
    shows: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize, Debug, Clone)]
struct Image {
    lg: String,
    md: String,
}

#[derive(Deserialize, Debug, BatchInserter)]
#[serde(rename_all = "camelCase")]
#[pgtable = "showtimes"]
struct Showtime {
    #[key]
    show_slug: Option<String>,
    #[key]
    cinema_slug: Option<String>,
    #[key]
    time: String,
    #[serde(rename = "refCmd")]
    reservation_url: String,
    #[key]
    auditorium_name: String,
    auditorium_capacity: String,
    end_time: String,
}

#[derive(Debug, Deserialize)]
struct RTResponse {
    results: Vec<RTResult>,
}

#[derive(Debug, Deserialize)]
struct RTResult {
    hits: Vec<RTHit>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RTHit {
    pub title: String,
    vanity: String,
    description: Option<String>,
    pub release_year: Option<i32>,
    pub rotten_tomatoes: Option<RTRating>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RTRating {
    audience_score: Option<i32>,
    score_sentiment: Option<String>,
    want_to_see_count: Option<i32>,
    critics_score: Option<i32>,
    certified_fresh: Option<bool>,
    new_adjusted_TM_score: Option<i32>,
}

#[derive(Debug, Deserialize, BatchInserter)]
#[pgtable = "ratings"]
pub struct Rating {
    #[key]
    slug: String,
    title: String,
    description: Option<String>,
    release_year: Option<i32>,
    audience_score: Option<i32>,
    score_sentiment: Option<String>,
    want_to_see_count: Option<i32>,
    critics_score: Option<i32>,
    certified_fresh: Option<bool>,
    new_adjusted_tm_score: Option<i32>,
}

#[derive(Debug, BatchInserter)]
#[pgtable = "ratingshows"]
pub struct RatingShow {
    #[key]
    show_slug: String,
    #[key]
    rating_slug: String,
    match_score: f64,
}

async fn fetch_cinema_shows(client: Client, cinema_slug: String) -> Result<Vec<String>> {
    let shows: CinemaShows = client
        .get_json(format!(
            "https://www.pathe.nl/api/cinema/{}/shows?language=nl",
            cinema_slug.clone()
        ))
        .await?;
    Ok(shows.shows.into_keys().collect())
}

async fn fetch_showtimes(
    client: Client,
    show_slug: String,
    cinema_slug: String,
) -> Result<Vec<Showtime>> {
    let request_url =
        format!("https://www.pathe.nl/api/show/{show_slug}/showtimes/{cinema_slug}?language=nl");
    let showtimes: HashMap<String, Vec<Showtime>> = match client.get_json(&request_url).await {
        Ok(res) => res,
        Err(JsonDecodeError::DecodeError(_)) => HashMap::default(),
        Err(JsonDecodeError::NetworkError(err)) => bail!(err),
    };
    Ok(showtimes
        .into_values()
        .flatten()
        .map(|mut showtime| {
            showtime.show_slug = Some(show_slug.clone());
            showtime.cinema_slug = Some(cinema_slug.clone());
            showtime
        })
        .collect())
}

async fn fetch_showtimes_cinema(client: Client, cinema: String) -> Result<Vec<Showtime>> {
    let mut handles = vec![];
    let shows = fetch_cinema_shows(client.clone(), cinema.clone()).await?;
    for show_slug in shows {
        handles.push(tokio::spawn(fetch_showtimes(
            client.clone(),
            show_slug,
            cinema.clone(),
        )));
    }
    let mut res = vec![];
    for handle in handles {
        let mut showtimes = handle.await??;
        res.append(&mut showtimes);
    }
    Ok(res)
}

async fn fetch_rt_data(client: Client, title: String) -> Result<RTResponse> {
    Ok(client.get_json_post("https://79frdp12pn-dsn.algolia.net/1/indexes/*/queries?x-algolia-agent=Algolia%20for%20JavaScript%20(4.24.0)%3B%20Browser%20(lite)&x-algolia-api-key=175588f6e5f8319b27702e4cc4013561&x-algolia-application-id=79FRDP12PN", 
    serde_json::json!({
		"requests": [
			{
				"indexName": "content_rt",
				"query": title,
				"params": "filters=isEmsSearchable%20%3D%201&hitsPerPage=5"
			}
		]
	})).await?)
}

pub async fn fetch_show_rating(
    client: Client,
    show_slug: String,
    title: String,
    year: Option<i32>,
) -> Result<Option<(Rating, RatingShow)>> {
    // TODO: NORMALIZE TITLE HERE BY REMOVING EVERYTHING BETWEEN PARENTHESES
    let rt_response = fetch_rt_data(client.clone(), title.clone()).await?;
    let best_hit = best_rt_hit(
        rt_response
            .results
            .into_iter()
            .next()
            .context("RTResponse should always return something")?
            .hits,
        title.clone(),
        year,
    );

    Ok(best_hit.map(|(hit, match_score)| {
        (
            Rating {
                slug: hit.vanity.clone(),
                title: hit.title,
                description: hit.description,
                release_year: hit.release_year,
                audience_score: hit
                    .rotten_tomatoes
                    .as_ref()
                    .and_then(|rt| rt.audience_score),
                score_sentiment: hit
                    .rotten_tomatoes
                    .as_ref()
                    .and_then(|rt| rt.score_sentiment.clone()),
                certified_fresh: hit
                    .rotten_tomatoes
                    .as_ref()
                    .and_then(|rt| rt.certified_fresh),
                want_to_see_count: hit
                    .rotten_tomatoes
                    .as_ref()
                    .and_then(|rt| rt.want_to_see_count),
                critics_score: hit.rotten_tomatoes.as_ref().and_then(|rt| rt.critics_score),
                new_adjusted_tm_score: hit
                    .rotten_tomatoes
                    .as_ref()
                    .and_then(|rt| rt.new_adjusted_TM_score),
            },
            RatingShow {
                show_slug,
                rating_slug: hit.vanity,
                match_score,
            },
        )
    }))
}

#[derive(Debug)]
pub struct MovieFetcher {
    pub pool: PgPool,
}
impl Runnable for MovieFetcher {
    async fn run(&self) -> Result<()> {
        let client = Client::new().with_limit(10.try_into()?).with_max_retries(3);
        let rt_client = Client::new().with_limit(10.try_into()?).with_max_retries(3);
        // Create inserters
        let mut showinserter = FlatShowInserter::new();
        let mut posterinserter = PosterInserter::new();
        let mut genreinserter = GenreInserter::new();
        let mut ratinginserter = RatingInserter::new();
        let mut ratingshowinserter = RatingShowInserter::new();

        // Fetch some basic information
        let (cinemas, cities, shows): (Vec<Cinema>, Vec<City>, Shows) = try_join!(
            client.get_json("https://www.pathe.nl/api/cinemas?language=nl"),
            client.get_json("https://www.pathe.nl/api/cities?language=nl"),
            client.get_json("https://www.pathe.nl/api/shows?language=nl")
        )?;

        let mut rating_handles = vec![];
        for (show, poster, genres) in shows.shows.into_iter().map(|show| show.flatten()) {
            rating_handles.push(tokio::spawn(fetch_show_rating(
                rt_client.clone(),
                show.slug.clone(),
                show.title.clone(),
                show.release_at.map(|date| date.year()),
            )));
            showinserter.add(show);
            posterinserter.add(poster);
            for genre in genres {
                genreinserter.add(genre);
            }
        }

        // Fetch showtimes
        let mut showtimes = vec![];
        let mut handles = vec![];
        for cinema in cinemas.iter().map(|cinema| cinema.slug.clone()) {
            handles.push(tokio::spawn(fetch_showtimes_cinema(client.clone(), cinema)));
        }

        // Join spawned tasks for showtimes
        for handle in handles {
            let mut cinema_showtimes = handle.await??;
            showtimes.append(&mut cinema_showtimes);
        }

        // Join spawned tasks for ratings
        let mut inserted_ratings = HashSet::new();
        for handle in rating_handles {
            let rating = handle.await??;
            if let Some((rating, ratingshow)) = rating {
                if !inserted_ratings.contains(&rating.slug) {
                    inserted_ratings.insert(rating.slug.clone());
                    ratinginserter.add(rating);
                }
                ratingshowinserter.add(ratingshow);
            }
        }

        CityInserter::from(cities)
            .build()
            .execute(&self.pool)
            .await?;

        CinemaInserter::from(cinemas)
            .build()
            .execute(&self.pool)
            .await?;

        showinserter.build().execute(&self.pool).await?;
        posterinserter.build().execute(&self.pool).await?;
        genreinserter.build().execute(&self.pool).await?;

        ShowtimeInserter::from(showtimes)
            .build()
            .execute(&self.pool)
            .await?;

        ratinginserter.build().execute(&self.pool).await?;
        ratingshowinserter.build().execute(&self.pool).await?;

        println!("Ran the fetcher for movies");
        Ok(())
    }
}
