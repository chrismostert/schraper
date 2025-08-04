use itertools::Itertools;
use strsim::normalized_levenshtein;

use crate::job::movies::RTHit;

pub fn best_rt_hit(hits: Vec<RTHit>, title: String, year: Option<i32>) -> Option<(RTHit, f64)> {
    hits.into_iter()
        .map(|hit| {
            let mut score = 0f64;

            // If we have year data, the absolute difference is used with a weighting
            if let Some(rt_year) = hit.release_year
                && let Some(pathe_year) = year
            {
                score += 0.1 * (rt_year as f64 - pathe_year as f64).abs();
            }

            // Most important for the score is the Levensthein distance between the
            // pathe title and the RT title
            score += 1f64 - normalized_levenshtein(&title, &hit.title);

            (hit, score)
        })
        .sorted_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
        .next()
}
