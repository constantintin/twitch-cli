use anyhow::{anyhow, bail, ensure, Context, Result};

use serde_json::Value;

use clap::clap_app;

use std::io::Read;
use std::io::{self, BufRead};
use std::process::{Command, Stdio};
use std::env;

#[derive(Debug)]
struct Channel {
    name: String,
    display_name: String,
    broadcaster_language: String,
    status: String,
}

#[derive(Debug)]
struct Stream {
    game: String,
    viewers: u64,
    channel: Channel,
}

#[derive(Debug)]
struct Game {
    name: String,
    viewers: u64,
}

trait Listable {
    fn name(&self) -> &String;
    fn viewers(&self) -> &u64;
    fn fields(&self) -> Vec<(String, String)>;
}

impl Listable for Game {
    fn name(&self) -> &String {
        &self.name
    }

    fn viewers(&self) -> &u64 {
        &self.viewers
    }

    fn fields(&self) -> Vec<(String, String)> {
        let fields = vec![
            (self.name.clone(), String::from("Name")),
            (self.viewers.to_string(), String::from("Viewers")),
        ];
        fields
    }
}

impl Listable for Stream {
    fn name(&self) -> &String {
        &self.channel.name
    }

    fn viewers(&self) -> &u64 {
        &self.viewers
    }

    fn fields(&self) -> Vec<(String, String)> {
        let fields = vec![
            (self.channel.name.clone(), String::from("Name")),
            (self.channel.status.clone(), String::from("Status")),
            (self.game.clone(), String::from("Game")),
            (self.viewers.to_string(), String::from("Viewers")),
        ];
        fields
    }
}

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
        Some(g) => watch_streams(g, info),
        None => match args.value_of("STREAM") {
            Some(s) => watch_channel(s, info),
            None => match args.is_present("FOLLOW") {
                true => watch_followed(info),
                false => watch_games(info),
            },
        },
    };
    match handle {
        Ok(_) => (),
        Err(e) => println!("{}", e),
    }
}

fn twitch_request(option: String, limit: i32) -> Result<Value> {
    let client = reqwest::blocking::Client::new();
    let url =
        "https://api.twitch.tv/helix/".to_string() + &option + "&limit=" + &limit.to_string();


    let access_token = env::var("TWITCH_ACCESS").context("Could not get access token, is TWITCH_ACCESS set?")?;
    let client_id = env::var("TWITCH_CLIENT_ID").context("Could not get client-id, is TWITCH_CLIENT_ID set?")?;

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
        let error_json: Value = serde_json::de::from_str(&body)
            .context(format!("Bad request. Url: {}, Status: {}", url, res.status()))?;
        match res.status() {
            reqwest::StatusCode::UNAUTHORIZED => bail!("Looks like no authorization string was supplied or it doesn't have required scope."),
            reqwest::StatusCode::NOT_FOUND => {
                let o_message = error_json.get("message");
                ensure!(!o_message.is_none(), "Bad request. Url: {}, Status: {}", url, res.status());

                let message = error_json
                    .get("message")
                    .and_then(|value| value.as_str())
                    .ok_or(anyhow!("Bad request. Url: {}, Status: {}", url, res.status()))?;
                if message.contains("Channel") {
                    let mut iter = message.split_whitespace();
                    iter.next();
                    bail!("The channel {} does not exist.", iter.next().unwrap().to_string());
                } else {
                    bail!("Bad request. Url: {}, Status: {}", url, res.status());
                }
            }
            _ => {
                bail!("Bad request. Url: {}, Status: {}", url, res.status());
            }
        }
    }

    Ok(serde_json::from_str(&body).unwrap())
}

fn twitch_streams(game: &str) -> Result<Vec<Stream>> {
    let requ: Value = twitch_request("streams?game=".to_string() + game, 10)?;
    ensure!(
        !requ.get("streams").expect("no streams in json").is_null(),
        "No streams available."
    );

    let streams_v = requ
        .get("streams")
        .expect("stream request parse error")
        .as_array()
        .expect("stream request parse error array");
    ensure!(streams_v.len() > 0, "No streams available");
    let streams = streams_v
        .iter()
        .map(|s| parse_stream(s).unwrap())
        .collect::<Vec<_>>();
    Ok(streams)
}
fn twitch_games() -> Result<Vec<Game>> {
    let requ: Value = twitch_request("games/top?".to_string(), 10)?;
    ensure!(
        !requ.get("streams").expect("no streams in json").is_null(),
        "No streams available."
    );

    let games_v = requ
        .get("top")
        .expect("game request parse error")
        .as_array()
        .expect("game request parse error array");
    ensure!(games_v.len() > 0, "No games available");
    let games = games_v
        .iter()
        .map(|g| parse_game(g).unwrap())
        .collect::<Vec<_>>();
    Ok(games)
}
fn twitch_followed() -> Result<Vec<Stream>> {
    let requ: Value = twitch_request("streams/followed".to_string() + "?", 10)?;
    ensure!(
        !requ.get("streams").expect("no streams in json").is_null(),
        "No streams available."
    );

    let streams_v = requ
        .get("streams")
        .expect("follow request parse error")
        .as_array()
        .expect("follow request parse error array");
    ensure!(streams_v.len() > 0, "No streams available");
    let streams = streams_v
        .iter()
        .map(|s| parse_stream(s).expect("no streams"))
        .collect::<Vec<_>>();
    Ok(streams)
}
fn twitch_channel(channel: &str) -> Result<Value> {
    twitch_request("streams/".to_string() + channel + "?", 0)
}

fn parse_stream(json: &Value) -> Result<Stream> {
    let channel_v = json.get("channel").ok_or(anyhow!("Offline."))?;

    let name = channel_v
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();
    let display_name = channel_v
        .get("display_name")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();
    let broadcaster_language = channel_v
        .get("broadcaster_language")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();
    let status = channel_v
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    let game = json
        .get("game")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();
    let viewers = json.get("viewers").and_then(|v| v.as_u64()).unwrap();

    let channel = Channel {
        name: name,
        display_name: display_name,
        broadcaster_language: broadcaster_language,
        status: status,
    };
    Ok(Stream {
        game: game,
        viewers: viewers,
        channel: channel,
    })
}

fn parse_game(json: &Value) -> Result<Game> {
    let viewers = json.get("viewers").and_then(|v| v.as_u64()).ok_or(anyhow!(
        "Error parsing json:\n {}.",
        serde_json::ser::to_string_pretty(&json.clone()).unwrap()
    ))?;
    let name = match &json["game"]["name"] {
        Value::String(s) => s,
        _ => bail!(
            "Error parsing json:\n {}.",
            serde_json::ser::to_string_pretty(&json.clone()).unwrap()
        ),
    };

    Ok(Game {
        name: name.to_string(),
        viewers: viewers,
    })
}

fn open_stream(stream: &Stream) -> Result<std::process::Child> {
    println!("Watching {}", stream.channel.name);
    let command = "/usr/local/bin/livestreamer";
    Command::new(command)
        .args(&[
            &*("https://www.twitch.tv/".to_string() + &*stream.channel.name),
            "best,720p60",
        ])
        .stdout(Stdio::null())
        .spawn()
        .context("Livestreamer has failed to execute. Is it properly installed and in you're path?")
}

fn watch_channel(name: &str, info: bool) -> Result<std::process::Child> {
    let channel = twitch_channel(name)?;
    let stream = channel
        .get("stream")
        .ok_or(anyhow!("No streams available."))?;
    match parse_stream(&stream) {
        Ok(s) => {
            if info {
                println!("Online");
                return Err(anyhow!(""));
            } else {
                open_stream(&s)
            }
        }
        Err(e) => Err(e),
    }
}

fn watch_streams(game: &str, info: bool) -> Result<std::process::Child> {
    let streams = twitch_streams(game)?;
    let sel_stream = choice(&streams, info)?;
    open_stream(&sel_stream)
}

fn watch_games(info: bool) -> Result<std::process::Child> {
    let games = twitch_games()?;
    let sel_game = choice(&games, info)?;

    watch_streams(&sel_game.name, false)
}

fn watch_followed(info: bool) -> Result<std::process::Child> {
    let streams = twitch_followed()?;
    let sel_stream = choice(&streams, info)?;

    open_stream(&sel_stream)
}

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
                .map(|fields| fields[i].0.len())
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
