use clap::ValueEnum;

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Item {
    #[value(alias = "covenant")]
    Cov,
    #[value(alias = "mystic")]
    Mys,
    #[value(alias = "friendship")]
    Fb,
}

impl Item {
    pub const ALL: [Item; 3] = [Item::Cov, Item::Mys, Item::Fb];

    pub fn name(self) -> &'static str {
        match self {
            Item::Cov => "Covenant bookmark",
            Item::Mys => "Mystic medal",
            Item::Fb => "Friendship points",
        }
    }

    pub fn gold(self) -> u32 {
        match self {
            Item::Cov => 184_000,
            Item::Mys => 280_000,
            Item::Fb => 18_000,
        }
    }

    pub fn asset(self) -> &'static str {
        match self {
            Item::Cov => "cov.png",
            Item::Mys => "mys.png",
            Item::Fb => "fb.png",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Item::Cov => "covenant",
            Item::Mys => "mystic",
            Item::Fb => "friendship",
        }
    }
}
