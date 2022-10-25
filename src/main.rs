mod types;

use anyhow::{anyhow, bail, Context, Result};

use serde_json::Value;

use clap::clap_app;

use unicode_segmentation::UnicodeSegmentation;

use std::env;
use std::io::Read;
use std::io::{self, BufRead};
use std::process::{Command, Stdio};

use types::{Stream, Game, Listable};

fn main() {
    let args = clap_app!(Twitch_cli =>
                         (version: "0.1")
                         (author: "Haddock")
                         (@arg GAME: -g --game +takes_value "Gets streams of game")
                         (@arg STREAM: -s --stream +takes_value "Gets stream if online")
                         (@arg FOLLOW: -f --follow "Gets followed streams")
                         (@arg INFO: -i --info "Only list info")
    )
    .get_matches();

    let info = args.is_present("INFO");

    let handle = match args.value_of("GAME") {
        Some(g) => watch_streams_by_game(g, info),
        None => match args.value_of("STREAM") {
            Some(s) => watch_channel(s),
            None => match args.is_present("FOLLOW") {
                true => watch_followed(info),
                false => watch_games(info),
            },
        },
    };
    match handle {
        Ok(_) => (),
        Err(e) => println!("{:?}", e),
    }
}

/// Make request to twitch api
/// returns: serde_json::Value of the response or Error
/// option: gets appended to url
/// must have TWITCH_ACCESS set to access token
/// and TWITCH_CLEINT_ID set to client-id for authentication
fn twitch_request(option: String) -> Result<Value> {
    let client = reqwest::blocking::Client::new();
    let url = "https://api.twitch.tv/helix/".to_string() + &option;

    let access_token =
        env::var("TWITCH_ACCESS").context("Could not get access token, is TWITCH_ACCESS set?")?;
    let client_id = env::var("TWITCH_CLIENT_ID")
        .context("Could not get client-id, is TWITCH_CLIENT_ID set?")?;

    let mut res = client
        .get(&*url)
        .header("Accept", "application/vnd.twitchtv.v3+json")
        .header("Authorization", &format!("Bearer {}", access_token))
        .header("Client-ID", &client_id)
        .send()
        .context("Could not connect to twitch api")?;

    let mut body: String = String::new();
    let _res_return = res
        .read_to_string(&mut body)
        .context("Reading response body into buffer failed")?;

    if res.status().is_client_error() {
        match res.status() {
            reqwest::StatusCode::UNAUTHORIZED => bail!("Looks like no authorization string was supplied or it doesn't have required scope."),
            reqwest::StatusCode::NOT_FOUND => bail!("Url '{}' was not found", url),
            _ => {
                bail!("Bad request. Url: {}, Status: {}", url, res.status());
            }
        }
    }

    Ok(serde_json::from_str(&body).unwrap())
}

/// make request to streams with specific game_id
/// returns streams in Vec
fn twitch_streams(game: &Game) -> Result<Vec<Stream>> {
    let requ: Value = twitch_request("streams?game_id=".to_string() + &game.id)?;

    let data = requ
        .get("data")
        .context(format!("No data in streams/{} response", game.name))?;

    let streams: Vec<Stream> =
        serde_json::from_value(data.clone()).context("Failed parsing streams")?;
    Ok(streams)
}

/// make request to top games
/// returns games in Vec
fn twitch_games() -> Result<Vec<Game>> {
    let requ: Value = twitch_request("games/top?".to_string())?;

    let data = requ.get("data").context("No data in games/top response")?;

    let games: Vec<Game> = serde_json::from_value(data.clone()).context("Failed parsing games")?;
    Ok(games)
}

/// make request game of specific name
/// returns first game returned or Error
/// used to get the game_id
fn twitch_game(name: &str) -> Result<Game> {
    let requ: Value = twitch_request("games?name=".to_string() + name)?;

    let data = requ
        .get("data")
        .with_context(|| format!("No streams for {}", name))?;

    let games: Vec<Game> =
        serde_json::from_value(data.clone()).context("Failed parsing streams")?;
    games
        .into_iter()
        .next()
        .ok_or(anyhow!("No streams for {}", name))
}

/// make request to followed streams
/// gets current user from Bearer token
/// returns streams in Vec
fn twitch_followed() -> Result<Vec<Stream>> {
    let current_user = twitch_request("users".to_string())?;
    let user_id = current_user
        .get("data")
        .ok_or(anyhow!("Twitch users request didn't have data"))?
        .get(0)
        .ok_or(anyhow!("Twitch users request didn't have users"))?
        .get("id")
        .ok_or(anyhow!("Twitch user didn't have id"))?
        .as_str()
        .ok_or(anyhow!("Twitch user id wasn't a string"))?;

    let requ: Value = twitch_request(format!("streams/followed?user_id={}", user_id))?;

    let data = requ
        .get("data")
        .context(format!("No data in followed streams response"))?;

    let streams: Vec<Stream> =
        serde_json::from_value(data.clone()).context("Failed parsing streams")?;
    Ok(streams)
}

/// spawn instance of streamlink
/// uses TWITCH_STREAMLINK_COMMAND or defaults to '/usr/local/bin/streamlink'
fn open_stream(stream: &Stream) -> Result<std::process::Child> {
    println!("Watching {}", stream.channel);
    let command = match env::var("TWITCH_STREAMLINK_COMMAND") {
        Ok(streamlink) => streamlink,
        Err(_) => "/usr/local/bin/streamlink".to_string(),
    };
    let stream = format!("https://www.twitch.tv/{}", stream.channel);
    println!("{}", stream);
    Command::new(command)
        .args(&[&stream, "best,720p60"])
        .stdout(Stdio::null())
        .spawn()
        .context("Streamlink has failed to execute. Is it properly installed and in you're path?")
}

/// watch stream for channel called 'name'
fn watch_channel(name: &str) -> Result<std::process::Child> {
    open_stream(&Stream {
        channel: name.to_string(),
        game: "".to_string(),
        viewers: 0,
    })
}

/// watch streams of game_name
fn watch_streams_by_game(game_name: &str, info: bool) -> Result<std::process::Child> {
    let game = twitch_game(game_name)?;
    watch_streams(&game, info)
}

/// watch streams of game
fn watch_streams(game: &Game, info: bool) -> Result<std::process::Child> {
    let streams = twitch_streams(game)?;
    let sel_stream = choice(&streams, info)?;
    open_stream(&sel_stream)
}

/// watch streams of top games
fn watch_games(info: bool) -> Result<std::process::Child> {
    let games = twitch_games()?;
    let sel_game = choice(&games, info)?;

    watch_streams(&sel_game, false)
}

/// watch streams of followed channels
fn watch_followed(info: bool) -> Result<std::process::Child> {
    let streams = twitch_followed()?;
    let sel_stream = choice(&streams, info)?;

    open_stream(&sel_stream)
}

/// present cli choice between elements of vec
/// or if info list all elements of vec
/// uses Listables fields() method
fn choice<T: Listable>(vec: &[T], info: bool) -> Result<&T> {
    let mut inputstr = String::new();
    let stdin = io::stdin();

    // Edge case where theres only one option
    if vec.len() == 1 && !info {
        loop {
            println!("Want to watch {}? [y/N]", vec[0].name());
            stdin
                .lock()
                .read_line(&mut inputstr)
                .context("Reading body into buffer failed.")?;
            match &inputstr.trim() as &str {
                "y" => return Ok(&vec[0]),
                "N" => return Err(anyhow!("")),
                _ => {
                    println!("Try again!\n");
                }
            }
            inputstr = String::new();
        }
    }

    let len = vec.len().to_string().len();

    let item_fields: Vec<Vec<(String, String)>> = vec.iter().map(|item| item.fields()).collect();

    let mut offsets = Vec::new();

    for i in 0..item_fields.iter().next().unwrap().len() {
        offsets.push(
            item_fields
                .iter()
                .map(|fields| fields[i].0.graphemes(true).count())
                .max()
                .unwrap(),
        );
    }

    if !info {
        println!(
            "Choose by typing the number next to the option [1 - {}]",
            vec.len()
        );
    }

    for _ in 0..len + 2 {
        print!(" ");
    }
    for field in item_fields
        .iter()
        .next()
        .unwrap()
        .iter()
        .zip(offsets.iter())
    {
        print!(
            "{field:<offset$}",
            field = (field.0).1,
            offset = field.1 + 3
        );
    }
    println!("");

    let mut i = 1;
    for fields in item_fields.iter() {
        print!("{i:>width$}) ", i = i, width = len);
        for field in fields.iter().zip(offsets.iter()) {
            print!("{field:<offset$}   ", field = (field.0).0, offset = field.1);
        }
        println!("");

        i += 1;
    }

    if info {
        bail!("");
    }

    loop {
        inputstr = String::new();
        stdin
            .lock()
            .read_line(&mut inputstr)
            .context("Reading body into buffer failed.")?;

        let input = inputstr.trim().parse::<i32>().context("Not a number")?;
        if input > vec.len() as i32 || input < 1 {
            println!("Try again!\n");
            continue;
        } else {
            return Ok(&vec[(input - 1) as usize]);
        }
    }
}
