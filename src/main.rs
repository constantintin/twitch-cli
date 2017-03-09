extern crate hyper;
extern crate serde_json;
use serde_json::Value;

#[macro_use]
extern crate clap;

use std::io::{self, BufRead};
use std::io::Read;

use std::fmt;

use std::process::{Command, Stdio};

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
}

impl Listable for Game {
    fn name (&self) -> &String {
        &self.name
    }

    fn viewers(&self) -> &u64 {
        &self.viewers
    }
}

impl Listable for Stream {
    fn name(&self) -> &String {
        &self.channel.name
    }

    fn viewers(&self) -> &u64 {
        &self.viewers
    }
}

type Result<T> = std::result::Result<T, TwitchError>;

#[derive(Debug)]
enum TwitchError {
    Hyper(hyper::error::Error),
    BadRequest { url: String, code: hyper::status::StatusCode },
    ReadBodyFailed(std::io::Error),
    NoAuthorizaion,
    BadChannel(String),
    StreamOffline,
    NoStreams,
    GameParseError(Value),
    LivestreamerFailed,
    NotNumber(std::num::ParseIntError),
    Info,
}



impl fmt::Display for TwitchError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            TwitchError::Hyper(ref e) =>
                write!(f, "Internet error occurred. Are you connected? Error:\n{}", e),
            TwitchError::BadRequest { ref url, ref code } => 
                write!(f, "Request URL failed.\nURL: '{}' Status Code: {}", url, code),
            TwitchError::ReadBodyFailed(ref e) =>
                write!(f, "Reading the body into a buffer failed\n Error: {}", e),
            TwitchError::NoAuthorizaion =>
                write!(f, "Looks like no authorization string was supplied or it doesn't have required scope"),
            TwitchError::BadChannel(ref c) =>
                write!(f, "The channel {} does not exist", c),
            TwitchError::StreamOffline =>
                write!(f, "Offline"),
            TwitchError::NoStreams =>
                write!(f, "No streams available"),
            TwitchError::GameParseError(ref j) =>
                write!(f, "Error parsing the following json:\n{}", serde_json::ser::to_string_pretty(&j).unwrap()),
            TwitchError::LivestreamerFailed =>
                write!(f, "Livestreamer has failed to execute. Is it properly installed and in you're path?"),
            TwitchError::NotNumber(ref e) =>
                write!(f, "That's not a number. Error:\n{}", e),
            TwitchError::Info =>
                write!(f, ""),
        }
    }
}

use std::error;

impl error::Error for TwitchError {
    fn description(&self) -> &str {
        match *self {
            TwitchError::Hyper(_) => "Internet Error",
            TwitchError::BadRequest { url: _, code: _ } => "Don't really know what went wrong...",
            TwitchError::ReadBodyFailed(ref e) => e.description(),
            TwitchError::NoAuthorizaion => "No Authorization",
            TwitchError::BadChannel(_) => "Channel doesn't exist",
            TwitchError::StreamOffline => "This stream is offline!",
            TwitchError::NoStreams => "No streams available",
            TwitchError::GameParseError(_) => "Failed parsing",
            TwitchError::LivestreamerFailed => "livestreamer failed",
            TwitchError::NotNumber(_) => "Not a number",
            TwitchError::Info => "Info",
        }
    }

    fn cause(&self) -> Option<&error::Error> {
        None
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
    ).get_matches();

    let info = args.is_present("INFO");
    
    let handle = match args.value_of("GAME") {
        Some(g) => watch_streams(g, info),
        None    => {
            match args.value_of("STREAM") {
                Some(s) => watch_channel(s, info),
                None => {
                    match args.is_present("FOLLOW") {
                        true => watch_followed(info),
                        false => watch_games(info),
                    }
                }
            }
        },
    }; 
    match handle {
        Ok(_) => (),
        Err(e) => println!("{}", e),
    }

    
}

fn twitch_request(option: String, limit: i32) -> Result<Value> {
    let client = hyper::Client::new();
    let mut headers = hyper::header::Headers::new();
    headers.set_raw("Authorization", vec![b"OAuth f96ge3agi90meg6c0y7ju3yak3r2uo".to_vec()]);
    headers.set_raw("Client-ID", vec![b"hqxa87yjzetn6wjgckdqxmmghdt9cqa".to_vec()]);
    let url = "https://api.twitch.tv/kraken/".to_string() + &option + "&limit=" + &limit.to_string();
    
    let mut res = try!(client
        .get(&*url)
        .headers(headers)
        .send()
        .map_err(|e| TwitchError::Hyper(e)));
    let mut body: String = String::new();
    let res_return = res.read_to_string(&mut body);

    match res_return {
        Err(e) =>
            return Err(TwitchError::ReadBodyFailed(e)),
        _ =>
            (),
    }

    if res.status.is_client_error() {
        let error_json: Value = try!(serde_json::de::from_str(&body)
                                     .map_err(|_| TwitchError::BadRequest { url: url.clone(), code: res.status } ));
        match res.status {
            hyper::status::StatusCode::Unauthorized =>
                return Err(TwitchError::NoAuthorizaion),
            hyper::status::StatusCode::NotFound => {
                let o_message = error_json.find("message");
                if o_message.is_none() {
                    return Err(TwitchError::BadRequest { url: url, code: res.status })
                }
                let message = try!(error_json
                                   .find("message")
                                   .and_then(|value| value.as_str())
                                   .ok_or(TwitchError::BadRequest { url: url.clone(), code: res.status } ) );
                if message.contains("Channel") {
                    let mut iter = message.split_whitespace();
                    iter.next();
                    return Err(TwitchError::BadChannel(iter.next().unwrap().to_string()))
                }
                else {
                    return Err(TwitchError::BadRequest { url: url, code: res.status })
                }
            },
            _ =>
                return Err(TwitchError::BadRequest { url: url, code: res.status }),
        }
    }

    Ok(serde_json::from_str(&body).unwrap())
}

fn twitch_streams(game: &str) -> Result<Vec<Stream>> {
    let requ: Value = try!(twitch_request("streams?game=".to_string() + game, 10));
    let streams_v = requ.find("streams")
        .expect("stream request parse error")
        .as_array()
        .expect("stream request parse error");
    if streams_v.len() == 0 {
        return Err(TwitchError::NoStreams)
    }
    let streams = streams_v
        .iter()
        .map(|s| parse_stream(s).unwrap())
        .collect::<Vec<_>>();
    Ok(streams)
}
fn twitch_games() -> Result<Vec<Game>> {
    let requ: Value = try!(twitch_request("games/top?".to_string(), 10));
    let games_v = requ.find("top")
        .expect("game request parse error")
        .as_array()
        .expect("game request parse error");
    if games_v.len() == 0 {
        return Err(TwitchError::NoStreams)
    }
    let games = games_v
        .iter()
        .map(|g| parse_game(g).unwrap())
        .collect::<Vec<_>>();
    Ok(games)
}
fn twitch_followed() -> Result<Vec<Stream>> {
    let requ: Value = try!(twitch_request("streams/followed".to_string() + "?", 10));
    let streams_v = requ.find("streams")
        .expect("follow request parse error")
        .as_array()
        .expect("follow request parse error");
    if streams_v.len() == 0 {
        return Err(TwitchError::NoStreams)
    }
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
    let channel_v = try!(json.find("channel").ok_or(TwitchError::StreamOffline));

    let name = channel_v.find("name")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();
    let display_name = channel_v.find("display_name")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();
    let broadcaster_language = channel_v.find("broadcaster_language")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();
    let status = channel_v.find("status")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    let game = json.find("game")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();
    let viewers = json.find("viewers")
        .and_then(|v| v.as_u64())
        .unwrap();

    let channel = Channel { name: name, display_name: display_name,
                            broadcaster_language: broadcaster_language, status: status };
    Ok( Stream { game: game, viewers: viewers, channel: channel } )
}

fn parse_game(json: &Value) -> Result<Game> {
    let viewers = try!(json.find("viewers")
                       .and_then(|v| v.as_u64())
                       .ok_or(TwitchError::GameParseError(json.clone())));
    let name = try!( json.find_path(&["game", "name"])
                     .and_then(|v| v.as_str())
                     .ok_or(TwitchError::GameParseError(json.clone()))
    ).to_string();

    Ok( Game { name: name, viewers: viewers })
}

fn open_stream(stream: &Stream) -> Result<std::process::Child> {
    println!("Watching {}", stream.channel.name);
    let command = "/usr/local/bin/livestreamer";
    Command::new(command)
        .args(&[&*("https://www.twitch.tv/".to_string() + &*stream.channel.name),
                "best,720p60"])
        .stdout(Stdio::null())
        .spawn()
        .map_err(|_| TwitchError::LivestreamerFailed)

}


fn watch_channel(name: &str, info: bool) -> Result<std::process::Child> {
    let channel = try!(twitch_channel(name));
    let stream = try!(channel.find("stream").ok_or(TwitchError::NoStreams));
    match parse_stream(&stream) {
        Ok(s) => if info { println!("Online"); return Err(TwitchError::Info)} else { open_stream(&s) },
        Err(e) => Err(e),
    }
}

fn watch_streams(game: &str, info: bool) -> Result<std::process::Child> {
    let streams = try!(twitch_streams(game));
    let sel_stream = try!(choice(&streams, info));
    open_stream(&sel_stream)
}

fn watch_games(info: bool) -> Result<std::process::Child> {
    let games = try!(twitch_games());
    let sel_game = try!(choice(&games, info));

    watch_streams(&sel_game.name, false)
}

fn watch_followed(info: bool) -> Result<std::process::Child> {
    let streams = try!(twitch_followed());
    let sel_stream = try!(choice(&streams, info));

    open_stream(&sel_stream)
}

fn choice<T: Listable>(vec: &[T], info: bool) -> Result<&T> {
    let mut inputstr = String::new();
    let stdin = io::stdin();

    // Edge case where theres only one option
    if vec.len() == 1 && !info {
        loop {
            println!("Want to watch {}? [y/N]", vec[0].name());
            try!(stdin
                 .lock()
                 .read_line(&mut inputstr)
                 .map_err(|e| TwitchError::ReadBodyFailed(e)));
            match &inputstr.trim() as &str {
                "y" => return Ok(&vec[0]),
                "N" => return Err(TwitchError::Info),
                _  => { println!("Try again!\n"); },
            }
            inputstr = String::new();
        }
    }

    let offset = vec
        .iter()
        .map(|item| item.name().len())
        .max()
        .unwrap();
    
    let len = vec
        .len()
        .to_string()
        .len();

    if !info {
        println!("Choose by typing the number next to the option [1 - {}]", vec.len());
    }
    
    let mut i = 1;
    for item in vec {
        println!("{i:>width1$}) {name:>width2$} {viewers}", i=i, width1=len, name=item.name(), width2=offset, viewers=item.viewers());
        i += 1;
    }

    if info {
        return Err(TwitchError::Info)
    }


    loop {
        inputstr = String::new();
        try!(stdin
             .lock()
             .read_line(&mut inputstr)
             .map_err(|e| TwitchError::ReadBodyFailed(e)));


        let input = try!(inputstr
                     .trim()
                     .parse::<i32>()
                     .map_err(|e| TwitchError::NotNumber(e)));
        if input > vec.len() as i32 || input < 1 {
            println!("Try again!\n");
            continue;
        }
        else {
            return Ok(&vec[(input - 1) as usize])
        }
    }
}
