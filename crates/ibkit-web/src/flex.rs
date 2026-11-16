//! Flex Web Service: download a Flex query statement as XML.
//!
//! `SendRequest?t=<token>&q=<query id>&v=3` answers with a reference code and
//! the statement URL; `GetStatement?q=<reference code>&t=<token>&v=3` answers
//! with the statement, or with `<Status>Warn</Status>` while IB still builds it.

use crate::client::{Result, WebError};
use std::time::Duration;

/// Statement period of a Flex request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlexPeriod<'a> {
    /// The period saved in the Flex query.
    #[default]
    Saved,
    /// `fd` to `td`, both `yyyyMMdd`.
    Range { from: &'a str, to: &'a str },
    /// The last `n` days (`p`, at most 365).
    Days(u16),
}

/// Default Flex Web Service base URL.
pub const FLEX_URL: &str = "https://ndcdyn.interactivebrokers.com/AccountManagement/FlexWebService";

/// Text of the first `<name>...</name>` element in `xml`.
fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let start = xml.find(&format!("<{name}>"))? + name.len() + 2;
    let end = start + xml[start..].find(&format!("</{name}>"))?;
    Some(xml[start..end].trim())
}

async fn get(http: &reqwest::Client, url: &str, query: &[(&str, &str)]) -> Result<String> {
    let body = crate::client::fetch("GET", url, http.get(url).query(query)).await?;
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// Download the statement of Flex query `query_id` for its saved period from
/// [`FLEX_URL`]; polls up to 10 times, 5 s apart, while it is being built.
pub async fn download(token: &str, query_id: &str) -> Result<String> {
    download_from(
        FLEX_URL,
        token,
        query_id,
        FlexPeriod::Saved,
        10,
        Duration::from_secs(5),
    )
    .await
}

/// [`download`] with an explicit base URL, period, number of polls and poll
/// interval.
pub async fn download_from(
    base_url: &str,
    token: &str,
    query_id: &str,
    period: FlexPeriod<'_>,
    polls: u32,
    wait: Duration,
) -> Result<String> {
    let days;
    let mut query = vec![("t", token), ("q", query_id), ("v", "3")];
    match period {
        FlexPeriod::Saved => {}
        FlexPeriod::Range { from, to } => query.extend([("fd", from), ("td", to)]),
        FlexPeriod::Days(n) => {
            if !(1..=365).contains(&n) {
                return Err(WebError::Invalid(format!(
                    "FlexPeriod::Days({n}) is out of range; the Flex Web Service accepts 1 to 365 days"
                )));
            }
            days = n.to_string();
            query.push(("p", &days));
        }
    }
    let http = reqwest::Client::builder()
        .user_agent("ibkit")
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| WebError::Http {
            url: base_url.into(),
            source: e,
        })?;
    let send_url = crate::client::join(base_url, "SendRequest");
    let answer = get(&http, &send_url, &query).await?;
    if tag(&answer, "Status") != Some("Success") {
        return Err(WebError::Invalid(format!(
            "Flex request for query {query_id} failed: {} {}; check the token and query id in Client Portal > Reports > Flex Queries",
            tag(&answer, "ErrorCode").unwrap_or("?"),
            tag(&answer, "ErrorMessage").unwrap_or(&answer),
        )));
    }
    let (Some(code), Some(url)) = (tag(&answer, "ReferenceCode"), tag(&answer, "Url")) else {
        return Err(WebError::Invalid(format!(
            "Flex request answer has no ReferenceCode or Url: {answer}"
        )));
    };
    for _ in 0..polls {
        let statement = get(&http, url, &[("q", code), ("t", token), ("v", "3")]).await?;
        // A statement is a <FlexQueryResponse>; a <FlexStatementResponse> is a status.
        if !statement.contains("<FlexStatementResponse") {
            return Ok(statement);
        }
        if tag(&statement, "Status") != Some("Warn") {
            return Err(WebError::Invalid(format!(
                "Flex statement for query {query_id} failed: {} {}",
                tag(&statement, "ErrorCode").unwrap_or("?"),
                tag(&statement, "ErrorMessage").unwrap_or(&statement),
            )));
        }
        tokio::time::sleep(wait).await;
    }
    Err(WebError::Invalid(format!(
        "the Flex statement for query {query_id} was not ready after {polls} polls {wait:?} apart; try again later or poll longer with download_from"
    )))
}
