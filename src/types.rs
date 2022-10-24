use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Stream {
    #[serde(rename = "game_name")]
    pub game: String,
    #[serde(rename = "viewer_count")]
    pub viewers: u64,
    #[serde(rename = "user_name")]
    pub channel: String,
}

#[derive(Debug, Deserialize)]
pub struct Game {
    pub name: String,
    pub id: String,
}

pub trait Listable {
    fn name(&self) -> String;
    fn fields(&self) -> Vec<(String, String)>;
}

impl Listable for Game {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn fields(&self) -> Vec<(String, String)> {
        let fields = vec![
            (self.name(), String::from("Name")),
        ];
        fields
    }
}

impl Listable for Stream {
    fn name(&self) -> String {
        self.channel.clone()
    }

    fn fields(&self) -> Vec<(String, String)> {
        let fields = vec![
            (self.name(), String::from("Name")),
            (self.game.clone(), String::from("Game")),
            (self.viewers.to_string(), String::from("Viewers")),
        ];
        fields
    }
}
