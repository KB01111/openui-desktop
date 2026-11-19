//! The app's own tools.
//!
//! `get_weather` is the template's example tool, ported as-is: Open-Meteo needs
//! no API key, so a fresh install has a working native tool loop.

use serde::Deserialize;
use serde_json::json;

use crate::openai::{FunctionSpec, ToolSpec};

pub const WEATHER_TOOL: &str = "get_weather";

const WEATHER_DESCRIPTION: &str =
    "Get the current weather for a city or place name. Use whenever the user \
     asks about weather, temperature, rain, or what to wear.";

pub fn specs() -> Vec<ToolSpec> {
    vec![ToolSpec {
        kind: "function",
        function: FunctionSpec {
            name: WEATHER_TOOL,
            description: WEATHER_DESCRIPTION,
            parameters: json!({
                "type": "object",
                "properties": {
                    "location": {
                        "type": "string",
                        "description": "City or place name, e.g. Berlin."
                    }
                },
                "required": ["location"],
                "additionalProperties": false
            }),
        },
    }]
}

/// Runs one tool call and returns its result as a string. Failures are values,
/// not errors: the model gets to explain the problem to the user.
pub async fn run(client: &reqwest::Client, name: &str, arguments: &str) -> String {
    match name {
        WEATHER_TOOL => match parse_location(arguments) {
            Ok(location) => match get_weather(client, &location).await {
                Ok(result) => result,
                Err(error) => json!({ "error": error }).to_string(),
            },
            Err(error) => json!({ "error": error }).to_string(),
        },
        other => json!({ "error": format!("Unknown tool: {other}") }).to_string(),
    }
}

#[derive(Deserialize)]
struct WeatherArgs {
    location: String,
}

fn parse_location(arguments: &str) -> Result<String, String> {
    let parsed: WeatherArgs = serde_json::from_str(arguments)
        .map_err(|error| format!("Invalid arguments for get_weather: {error}"))?;
    let location = parsed.location.trim().to_string();
    if location.is_empty() {
        return Err("location is required".to_string());
    }
    Ok(location)
}

/// WMO weather interpretation codes, banded exactly as the web template did.
pub fn describe_weather(code: u16) -> &'static str {
    match code {
        0 => "clear sky",
        1..=3 => "partly cloudy",
        4..=48 => "fog",
        49..=57 => "drizzle",
        58..=67 => "rain",
        68..=77 => "snow",
        78..=82 => "rain showers",
        83..=86 => "snow showers",
        _ => "thunderstorm",
    }
}

#[derive(Deserialize)]
struct GeoResponse {
    #[serde(default)]
    results: Vec<GeoPlace>,
}

#[derive(Deserialize)]
struct GeoPlace {
    name: String,
    #[serde(default)]
    country: Option<String>,
    latitude: f64,
    longitude: f64,
}

#[derive(Deserialize)]
struct ForecastResponse {
    #[serde(default)]
    current: Option<CurrentWeather>,
}

#[derive(Deserialize)]
struct CurrentWeather {
    temperature_2m: f64,
    weather_code: u16,
    wind_speed_10m: f64,
}

async fn get_json<T: for<'de> Deserialize<'de>>(
    client: &reqwest::Client,
    url: reqwest::Url,
) -> Result<T, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("Request failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("Open-Meteo returned {}", response.status()));
    }
    response
        .json::<T>()
        .await
        .map_err(|error| format!("Response was not valid JSON: {error}"))
}

pub async fn get_weather(client: &reqwest::Client, location: &str) -> Result<String, String> {
    let geo_url = reqwest::Url::parse_with_params(
        "https://geocoding-api.open-meteo.com/v1/search",
        &[("name", location), ("count", "1")],
    )
    .map_err(|error| format!("Could not build the geocoding URL: {error}"))?;

    let geo: GeoResponse = get_json(client, geo_url).await?;
    let place = geo
        .results
        .into_iter()
        .next()
        .ok_or_else(|| format!("No place found for \"{location}\""))?;

    let forecast_url = reqwest::Url::parse_with_params(
        "https://api.open-meteo.com/v1/forecast",
        &[
            ("latitude", place.latitude.to_string()),
            ("longitude", place.longitude.to_string()),
            (
                "current",
                "temperature_2m,weather_code,wind_speed_10m".to_string(),
            ),
        ],
    )
    .map_err(|error| format!("Could not build the forecast URL: {error}"))?;

    let forecast: ForecastResponse = get_json(client, forecast_url).await?;
    let current = forecast
        .current
        .ok_or_else(|| "No weather data returned".to_string())?;

    let label = match place.country {
        Some(country) if !country.is_empty() => format!("{}, {country}", place.name),
        _ => place.name,
    };

    Ok(json!({
        "place": label,
        "temperature_c": current.temperature_2m,
        "conditions": describe_weather(current.weather_code),
        "wind_kmh": current.wind_speed_10m,
    })
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weather_codes_match_the_web_template() {
        assert_eq!(describe_weather(0), "clear sky");
        assert_eq!(describe_weather(2), "partly cloudy");
        assert_eq!(describe_weather(45), "fog");
        assert_eq!(describe_weather(53), "drizzle");
        assert_eq!(describe_weather(63), "rain");
        assert_eq!(describe_weather(71), "snow");
        assert_eq!(describe_weather(80), "rain showers");
        assert_eq!(describe_weather(85), "snow showers");
        assert_eq!(describe_weather(95), "thunderstorm");
    }

    #[test]
    fn location_is_required() {
        assert!(parse_location(r#"{"location":"Berlin"}"#).is_ok());
        assert!(parse_location(r#"{"location":"   "}"#).is_err());
        assert!(parse_location("{}").is_err());
        assert!(parse_location("not json").is_err());
    }

    #[tokio::test]
    async fn unknown_tools_report_an_error_value() {
        let client = reqwest::Client::new();
        let result = run(&client, "nope", "{}").await;
        assert!(result.contains("Unknown tool: nope"));
    }
}
