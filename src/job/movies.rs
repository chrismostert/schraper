use std::collections::HashMap;

use crate::job::util::JsonDecodeError;

use super::{util::Client, Runnable};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use sqlx::PgPool;
use tokio::try_join;

use sqlx_batch::BatchInserter;

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
                release_at: self.release_at.into_iter().next(),
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
    release_at: Option<String>,
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
struct RTHit {
    title: String,
    vanity: String,
    description: Option<String>,
    release_year: Option<i32>,
    rotten_tomatoes: Option<RTRating>,
}

#[derive(Debug, Deserialize, BatchInserter)]
pub struct ShowRating {
    slug: String,
    show_slug: String,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RTRating {
    audience_score: Option<i32>,
    score_sentiment: Option<String>,
    want_to_see_count: Option<i32>,
    critics_score: Option<i32>,
    certified_fresh: Option<bool>,
    new_adjusted_TM_score: Option<i32>,
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
    year: Option<String>,
) -> Result<Option<ShowRating>> {
    let rt_response = fetch_rt_data(client.clone(), title.clone()).await?;

    // Todo select best hit
    let best_hit = rt_response
        .results
        .into_iter()
        .next()
        .context("No results returned")?
        .hits
        .into_iter()
        .next();

    if best_hit.is_none() {
        return Ok(None);
    }
    let best_hit = best_hit.unwrap();

    let (
        audience_score,
        score_sentiment,
        want_to_see_count,
        critics_score,
        certified_fresh,
        new_adjusted_tm_score,
    ) = if let Some(rt) = best_hit.rotten_tomatoes {
        (
            rt.audience_score,
            rt.score_sentiment,
            rt.want_to_see_count,
            rt.critics_score,
            rt.certified_fresh,
            rt.new_adjusted_TM_score,
        )
    } else {
        (None, None, None, None, None, None)
    };

    Ok(Some(ShowRating {
        slug: best_hit.vanity,
        show_slug,
        title,
        description: best_hit.description,
        release_year: best_hit.release_year,
        audience_score,
        score_sentiment,
        want_to_see_count,
        critics_score,
        certified_fresh,
        new_adjusted_tm_score,
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
        let mut ratinginserter = ShowRatingInserter::new();

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
                show.release_at.clone(),
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

        // Join spawned tasks
        for handle in handles {
            let mut cinema_showtimes = handle.await??;
            showtimes.append(&mut cinema_showtimes);
        }
        for handle in rating_handles {
            let rating = handle.await??;
            if let Some(rating) = rating {
                ratinginserter.add(rating);
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

        // Write ratings to database

        println!("Ran the fetcher for movies");
        Ok(())
    }
}
