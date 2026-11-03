//! Каталог видов предметов (GAME_DESIGN 3.7): `ItemKind` со стабильными
//! строковыми id, классы предметов и частей тела, инструменты/оружие,
//! горючесть и питательность. Данные, а не логика — в перспективе каталог
//! переезжает в `data/`. Слоты и инвентарь — в родительском модуле.

use crate::simulation::tile::ResourceKind;
use super::{BodySlot, ItemClass, MAX_STACK};

/// Число видов предметов (иконки UI, текстуры, массивы отображения).
pub const ITEM_KIND_COUNT: usize = 90;

/// Вид предмета. Значения — стабильные идентификаторы (repr(u8));
/// новые виды добавляются только в конец, чтобы не сдвигать существующие.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ItemKind {
    /// Палка (из кустов, побочно из деревьев).
    Stick = 0,
    /// Камень (откалывается от валунов).
    Rock,
    /// Яблоко (с яблони, ПКМ).
    Apple,
    /// Пучок травы (с травяного тайла).
    GrassBunch,
    /// Растительное волокно (крафт из травы).
    PlantFiber,
    /// Верёвка из волокон (крафт).
    Cordage,
    /// Примитивный топор.
    AxePrimal,
    /// Примитивная кирка.
    PickaxePrimal,
    /// Примитивная мотыга.
    HoePrimal,
    /// Примитивная лопата.
    ShovelPrimal,
    /// Примитивное копьё.
    SpearPrimal,
    /// Каменный нож.
    KnifePrimal,
    /// Примитивная удочка.
    FishingRod,
    /// Стрела (боеприпас лука).
    Arrow,
    /// Деревянный лук.
    Bow,
    /// Кольчужный шлем (крафтов пока нет).
    HelmetChainmail,
    /// Кольчужная кираса.
    ChestplateChainmail,
    /// Кольчужные поножи.
    LeggingsChainmail,
    /// Кольчужные ботинки.
    BootsChainmail,
    /// Кость (задел под костяные инструменты и оружие).
    Bone,
    /// Осколок кости.
    ShardBone,
    /// Оперение стрелы.
    ArrowFletching,
    /// Перо.
    Feather,
    /// Нить.
    String,
    /// Связка палок (8 палок, крафт и обратное развязывание).
    StickBundle,
    /// Книга (задел под передачу знаний).
    Book,
    /// Бумага.
    Paper,
    /// Бумажная масса (задел под крафт бумаги).
    PaperPulp,
    /// Комок глины.
    ClayClod,
    /// Сырой глиняный кирпич (задел под обжиг).
    ClayBrickUnfired,
    /// Обожжённый глиняный кирпич.
    ClayBrickBaked,
    /// Семена пшеницы.
    WheatSeeds,
    /// Пшеница.
    Wheat,
    /// Каменный молот.
    HammerStone,
    /// Медный самородок.
    NuggetCopper,
    /// Золотой самородок.
    NuggetGold,
    /// Серебряный самородок.
    NuggetSilver,
    /// Медный слиток.
    IngotCopper,
    /// Бронзовый слиток.
    IngotBronze,
    /// Оловянный слиток.
    IngotTin,
    /// Золотой слиток.
    IngotGold,
    /// Серебряный слиток.
    IngotSilver,
    /// Кварц.
    GemQuartz,
    /// Кремень.
    GemFlint,
    /// Изумруд.
    GemEmerald,
    /// Рубин.
    GemRuby,
    /// Бронзовый порошок.
    DustBronze,
    /// Угольный порошок.
    DustCharcoal,
    /// Медный порошок.
    DustCopper,
    /// Мука.
    DustFlour,
    /// Золотой порошок.
    DustGold,
    /// Пепел.
    DustPitAsh,
    /// Серебряный порошок.
    DustSilver,
    /// Сахар.
    DustSugar,
    /// Сера.
    DustSulfur,
    /// Оловянный порошок.
    DustTin,
    /// Опилки.
    DustWoodChips,
    /// Печёное яблоко (готовка на костре).
    AppleBaked,
    /// Морковь.
    Carrot,
    /// Печёная морковь (готовка на костре).
    CarrotBaked,
    /// Яйцо.
    Egg,
    /// Жареное яйцо (готовка на костре).
    EggRoasted,
    /// Сало.
    Lard,
    /// Сырое мясо птицы.
    MeatBirdRaw,
    /// Жареное мясо птицы (готовка на костре).
    MeatBirdCooked,
    /// Сырое мясо травоядного.
    MeatHerbivoreRaw,
    /// Жареное мясо травоядного (готовка на костре).
    MeatHerbivoreCooked,
    /// Сырое мясо хищника.
    MeatPredatorRaw,
    /// Жареное мясо хищника (готовка на костре).
    MeatPredatorCooked,
    /// Картофель.
    Potato,
    /// Печёный картофель (готовка на костре).
    PotatoBaked,
    /// Странный клубень (задел: добыча лопатой из травы/земли).
    StrangeTuber,
    /// Медная руда.
    RawCopper,
    /// Золотая руда.
    RawGold,
    /// Серебряная руда.
    RawSilver,
    /// Оловянная руда.
    RawTin,
    /// Медный топор (крафтов пока нет).
    AxeCopper,
    /// Медный молот.
    HammerCopper,
    /// Медная мотыга.
    HoeCopper,
    /// Медная кирка.
    PickaxeCopper,
    /// Медная пила.
    SawCopper,
    /// Медные ножницы.
    ShearsCopper,
    /// Медная лопата.
    ShovelCopper,
    /// Медный серп.
    SickleCopper,
    /// Семена свёклы.
    BeetrootSeeds,
    /// Свёкла.
    Beetroot,
    /// Печёная свёкла (готовка в печи).
    BeetrootRoasted,
    /// Вишня (горсть ягод).
    Cherry,
    /// Уголь (топливо плавильной печи).
    Coal,
    /// Древесный уголь (топливо печи).
    Charcoal,
}

impl ItemKind {
    /// Все виды предметов в стабильном порядке (UI, тесты).
    pub const ALL: [ItemKind; ITEM_KIND_COUNT] = [
        ItemKind::Stick,
        ItemKind::Rock,
        ItemKind::Apple,
        ItemKind::GrassBunch,
        ItemKind::PlantFiber,
        ItemKind::Cordage,
        ItemKind::AxePrimal,
        ItemKind::PickaxePrimal,
        ItemKind::HoePrimal,
        ItemKind::ShovelPrimal,
        ItemKind::SpearPrimal,
        ItemKind::KnifePrimal,
        ItemKind::FishingRod,
        ItemKind::Arrow,
        ItemKind::Bow,
        ItemKind::HelmetChainmail,
        ItemKind::ChestplateChainmail,
        ItemKind::LeggingsChainmail,
        ItemKind::BootsChainmail,
        ItemKind::Bone,
        ItemKind::ShardBone,
        ItemKind::ArrowFletching,
        ItemKind::Feather,
        ItemKind::String,
        ItemKind::StickBundle,
        ItemKind::Book,
        ItemKind::Paper,
        ItemKind::PaperPulp,
        ItemKind::ClayClod,
        ItemKind::ClayBrickUnfired,
        ItemKind::ClayBrickBaked,
        ItemKind::WheatSeeds,
        ItemKind::Wheat,
        ItemKind::HammerStone,
        ItemKind::NuggetCopper,
        ItemKind::NuggetGold,
        ItemKind::NuggetSilver,
        ItemKind::IngotCopper,
        ItemKind::IngotBronze,
        ItemKind::IngotTin,
        ItemKind::IngotGold,
        ItemKind::IngotSilver,
        ItemKind::GemQuartz,
        ItemKind::GemFlint,
        ItemKind::GemEmerald,
        ItemKind::GemRuby,
        ItemKind::DustBronze,
        ItemKind::DustCharcoal,
        ItemKind::DustCopper,
        ItemKind::DustFlour,
        ItemKind::DustGold,
        ItemKind::DustPitAsh,
        ItemKind::DustSilver,
        ItemKind::DustSugar,
        ItemKind::DustSulfur,
        ItemKind::DustTin,
        ItemKind::DustWoodChips,
        ItemKind::AppleBaked,
        ItemKind::Carrot,
        ItemKind::CarrotBaked,
        ItemKind::Egg,
        ItemKind::EggRoasted,
        ItemKind::Lard,
        ItemKind::MeatBirdRaw,
        ItemKind::MeatBirdCooked,
        ItemKind::MeatHerbivoreRaw,
        ItemKind::MeatHerbivoreCooked,
        ItemKind::MeatPredatorRaw,
        ItemKind::MeatPredatorCooked,
        ItemKind::Potato,
        ItemKind::PotatoBaked,
        ItemKind::StrangeTuber,
        ItemKind::RawCopper,
        ItemKind::RawGold,
        ItemKind::RawSilver,
        ItemKind::RawTin,
        ItemKind::AxeCopper,
        ItemKind::HammerCopper,
        ItemKind::HoeCopper,
        ItemKind::PickaxeCopper,
        ItemKind::SawCopper,
        ItemKind::ShearsCopper,
        ItemKind::ShovelCopper,
        ItemKind::SickleCopper,
        ItemKind::BeetrootSeeds,
        ItemKind::Beetroot,
        ItemKind::BeetrootRoasted,
        ItemKind::Cherry,
        ItemKind::Coal,
        ItemKind::Charcoal,
    ];

    /// Вид предмета по стабильному значению u8 (сохранения).
    #[allow(dead_code)] // задел для сохранений, как у остальных repr(u8)
    pub fn from_discriminant(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Stick),
            1 => Some(Self::Rock),
            2 => Some(Self::Apple),
            3 => Some(Self::GrassBunch),
            4 => Some(Self::PlantFiber),
            5 => Some(Self::Cordage),
            6 => Some(Self::AxePrimal),
            7 => Some(Self::PickaxePrimal),
            8 => Some(Self::HoePrimal),
            9 => Some(Self::ShovelPrimal),
            10 => Some(Self::SpearPrimal),
            11 => Some(Self::KnifePrimal),
            12 => Some(Self::FishingRod),
            13 => Some(Self::Arrow),
            14 => Some(Self::Bow),
            15 => Some(Self::HelmetChainmail),
            16 => Some(Self::ChestplateChainmail),
            17 => Some(Self::LeggingsChainmail),
            18 => Some(Self::BootsChainmail),
            19 => Some(Self::Bone),
            20 => Some(Self::ShardBone),
            21 => Some(Self::ArrowFletching),
            22 => Some(Self::Feather),
            23 => Some(Self::String),
            24 => Some(Self::StickBundle),
            25 => Some(Self::Book),
            26 => Some(Self::Paper),
            27 => Some(Self::PaperPulp),
            28 => Some(Self::ClayClod),
            29 => Some(Self::ClayBrickUnfired),
            30 => Some(Self::ClayBrickBaked),
            31 => Some(Self::WheatSeeds),
            32 => Some(Self::Wheat),
            33 => Some(Self::HammerStone),
            34 => Some(Self::NuggetCopper),
            35 => Some(Self::NuggetGold),
            36 => Some(Self::NuggetSilver),
            37 => Some(Self::IngotCopper),
            38 => Some(Self::IngotBronze),
            39 => Some(Self::IngotTin),
            40 => Some(Self::IngotGold),
            41 => Some(Self::IngotSilver),
            42 => Some(Self::GemQuartz),
            43 => Some(Self::GemFlint),
            44 => Some(Self::GemEmerald),
            45 => Some(Self::GemRuby),
            46 => Some(Self::DustBronze),
            47 => Some(Self::DustCharcoal),
            48 => Some(Self::DustCopper),
            49 => Some(Self::DustFlour),
            50 => Some(Self::DustGold),
            51 => Some(Self::DustPitAsh),
            52 => Some(Self::DustSilver),
            53 => Some(Self::DustSugar),
            54 => Some(Self::DustSulfur),
            55 => Some(Self::DustTin),
            56 => Some(Self::DustWoodChips),
            57 => Some(Self::AppleBaked),
            58 => Some(Self::Carrot),
            59 => Some(Self::CarrotBaked),
            60 => Some(Self::Egg),
            61 => Some(Self::EggRoasted),
            62 => Some(Self::Lard),
            63 => Some(Self::MeatBirdRaw),
            64 => Some(Self::MeatBirdCooked),
            65 => Some(Self::MeatHerbivoreRaw),
            66 => Some(Self::MeatHerbivoreCooked),
            67 => Some(Self::MeatPredatorRaw),
            68 => Some(Self::MeatPredatorCooked),
            69 => Some(Self::Potato),
            70 => Some(Self::PotatoBaked),
            71 => Some(Self::StrangeTuber),
            72 => Some(Self::RawCopper),
            73 => Some(Self::RawGold),
            74 => Some(Self::RawSilver),
            75 => Some(Self::RawTin),
            76 => Some(Self::AxeCopper),
            77 => Some(Self::HammerCopper),
            78 => Some(Self::HoeCopper),
            79 => Some(Self::PickaxeCopper),
            80 => Some(Self::SawCopper),
            81 => Some(Self::ShearsCopper),
            82 => Some(Self::ShovelCopper),
            83 => Some(Self::SickleCopper),
            84 => Some(Self::BeetrootSeeds),
            85 => Some(Self::Beetroot),
            86 => Some(Self::BeetrootRoasted),
            87 => Some(Self::Cherry),
            88 => Some(Self::Coal),
            89 => Some(Self::Charcoal),
            _ => None,
        }
    }

    /// Стабильный строковый идентификатор для JSON-рецептов.
    pub fn id(self) -> &'static str {
        match self {
            ItemKind::Stick => "stick",
            ItemKind::Rock => "rock",
            ItemKind::Apple => "apple",
            ItemKind::GrassBunch => "grass_bunch",
            ItemKind::PlantFiber => "plant_fiber",
            ItemKind::Cordage => "cordage",
            ItemKind::AxePrimal => "primal_axe",
            ItemKind::PickaxePrimal => "primal_pickaxe",
            ItemKind::HoePrimal => "primal_hoe",
            ItemKind::ShovelPrimal => "primal_shovel",
            ItemKind::SpearPrimal => "primal_spear",
            ItemKind::KnifePrimal => "primal_knife",
            ItemKind::FishingRod => "fishing_rod",
            ItemKind::Arrow => "arrow",
            ItemKind::Bow => "bow",
            ItemKind::HelmetChainmail => "helmet_chainmail",
            ItemKind::ChestplateChainmail => "chestplate_chainmail",
            ItemKind::LeggingsChainmail => "leggings_chainmail",
            ItemKind::BootsChainmail => "boots_chainmail",
            ItemKind::Bone => "bone",
            ItemKind::ShardBone => "shard_bone",
            ItemKind::ArrowFletching => "arrow_fletching",
            ItemKind::Feather => "feather",
            ItemKind::String => "string",
            ItemKind::StickBundle => "stick_bundle",
            ItemKind::Book => "book",
            ItemKind::Paper => "paper",
            ItemKind::PaperPulp => "paper_pulp",
            ItemKind::ClayClod => "clod_clay",
            ItemKind::ClayBrickUnfired => "brick_clay_unfired",
            ItemKind::ClayBrickBaked => "brick_clay_baked",
            ItemKind::WheatSeeds => "seeds_wheat",
            ItemKind::Wheat => "wheat",
            ItemKind::HammerStone => "hammer_stone",
            ItemKind::NuggetCopper => "nugget_copper",
            ItemKind::NuggetGold => "nugget_gold",
            ItemKind::NuggetSilver => "nugget_silver",
            ItemKind::IngotCopper => "ingot_copper",
            ItemKind::IngotBronze => "ingot_bronze",
            ItemKind::IngotTin => "ingot_tin",
            ItemKind::IngotGold => "ingot_gold",
            ItemKind::IngotSilver => "ingot_silver",
            ItemKind::GemQuartz => "gem_quartz",
            ItemKind::GemFlint => "gem_flint",
            ItemKind::GemEmerald => "gem_emerald",
            ItemKind::GemRuby => "gem_ruby",
            ItemKind::DustBronze => "dust_bronze",
            ItemKind::DustCharcoal => "dust_charcoal",
            ItemKind::DustCopper => "dust_copper",
            ItemKind::DustFlour => "dust_flour",
            ItemKind::DustGold => "dust_gold",
            ItemKind::DustPitAsh => "dust_pit_ash",
            ItemKind::DustSilver => "dust_silver",
            ItemKind::DustSugar => "dust_sugar",
            ItemKind::DustSulfur => "dust_sulfur",
            ItemKind::DustTin => "dust_tin",
            ItemKind::DustWoodChips => "dust_wood_chips",
            ItemKind::AppleBaked => "apple_baked",
            ItemKind::Carrot => "carrot",
            ItemKind::CarrotBaked => "carrot_baked",
            ItemKind::Egg => "egg",
            ItemKind::EggRoasted => "egg_roasted",
            ItemKind::Lard => "lard",
            ItemKind::MeatBirdRaw => "meat_bird_raw",
            ItemKind::MeatBirdCooked => "meat_bird_cooked",
            ItemKind::MeatHerbivoreRaw => "meat_herbivore_raw",
            ItemKind::MeatHerbivoreCooked => "meat_herbivore_cooked",
            ItemKind::MeatPredatorRaw => "meat_predator_raw",
            ItemKind::MeatPredatorCooked => "meat_predator_cooked",
            ItemKind::Potato => "potato",
            ItemKind::PotatoBaked => "potato_baked",
            ItemKind::StrangeTuber => "strange_tuber",
            ItemKind::RawCopper => "raw_copper",
            ItemKind::RawGold => "raw_gold",
            ItemKind::RawSilver => "raw_silver",
            ItemKind::RawTin => "raw_tin",
            ItemKind::AxeCopper => "axe_copper",
            ItemKind::HammerCopper => "hammer_copper",
            ItemKind::HoeCopper => "hoe_copper",
            ItemKind::PickaxeCopper => "pickaxe_copper",
            ItemKind::SawCopper => "saw_copper",
            ItemKind::ShearsCopper => "shears_copper",
            ItemKind::ShovelCopper => "shovel_copper",
            ItemKind::SickleCopper => "sickle_copper",
            ItemKind::BeetrootSeeds => "seeds_beetroot",
            ItemKind::Beetroot => "beetroot",
            ItemKind::BeetrootRoasted => "beetroot_roasted",
            ItemKind::Cherry => "cherry",
            ItemKind::Coal => "coal",
            ItemKind::Charcoal => "charcoal",
        }
    }

    /// Вид предмета по строковому идентификатору из JSON.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.id() == id)
    }

    /// Предмет, в который превращается добытый ресурс мира. `None` — у
    /// ресурса пока нет предмета (руда, глина, дерево появятся вместе
    /// с производством).
    pub fn from_resource(kind: ResourceKind) -> Option<Self> {
        match kind {
            ResourceKind::Stone => Some(Self::Rock),
            ResourceKind::Stick => Some(Self::Stick),
            _ => None,
        }
    }

    /// Класс предмета: определяет, в какие слоты его можно класть
    /// (и к какой секции он тяготеет). Match без wildcard: добавление
    /// нового вида не компилируется, пока класс не решён явно.
    pub fn class(self) -> ItemClass {
        match self {
            ItemKind::Stick
            | ItemKind::Rock
            | ItemKind::Apple
            | ItemKind::GrassBunch
            | ItemKind::PlantFiber
            | ItemKind::Cordage
            | ItemKind::AxePrimal
            | ItemKind::PickaxePrimal
            | ItemKind::HoePrimal
            | ItemKind::ShovelPrimal
            | ItemKind::SpearPrimal
            | ItemKind::KnifePrimal
            | ItemKind::FishingRod
            | ItemKind::Arrow
            | ItemKind::Bow
            | ItemKind::Bone
            | ItemKind::ShardBone
            | ItemKind::ArrowFletching
            | ItemKind::Feather
            | ItemKind::String
            | ItemKind::StickBundle
            | ItemKind::Book
            | ItemKind::Paper
            | ItemKind::PaperPulp
            | ItemKind::ClayClod
            | ItemKind::ClayBrickUnfired
            | ItemKind::ClayBrickBaked
            | ItemKind::WheatSeeds
            | ItemKind::Wheat
            | ItemKind::HammerStone
            | ItemKind::NuggetCopper
            | ItemKind::NuggetGold
            | ItemKind::NuggetSilver
            | ItemKind::IngotCopper
            | ItemKind::IngotBronze
            | ItemKind::IngotTin
            | ItemKind::IngotGold
            | ItemKind::IngotSilver
            | ItemKind::GemQuartz
            | ItemKind::GemFlint
            | ItemKind::GemEmerald
            | ItemKind::GemRuby
            | ItemKind::DustBronze
            | ItemKind::DustCharcoal
            | ItemKind::DustCopper
            | ItemKind::DustFlour
            | ItemKind::DustGold
            | ItemKind::DustPitAsh
            | ItemKind::DustSilver
            | ItemKind::DustSugar
            | ItemKind::DustSulfur
            | ItemKind::DustTin
            | ItemKind::DustWoodChips
            | ItemKind::AppleBaked
            | ItemKind::Carrot
            | ItemKind::CarrotBaked
            | ItemKind::Egg
            | ItemKind::EggRoasted
            | ItemKind::Lard
            | ItemKind::MeatBirdRaw
            | ItemKind::MeatBirdCooked
            | ItemKind::MeatHerbivoreRaw
            | ItemKind::MeatHerbivoreCooked
            | ItemKind::MeatPredatorRaw
            | ItemKind::MeatPredatorCooked
            | ItemKind::Potato
            | ItemKind::PotatoBaked
            | ItemKind::StrangeTuber
            | ItemKind::RawCopper
            | ItemKind::RawGold
            | ItemKind::RawSilver
            | ItemKind::RawTin
            | ItemKind::AxeCopper
            | ItemKind::HammerCopper
            | ItemKind::HoeCopper
            | ItemKind::PickaxeCopper
            | ItemKind::SawCopper
            | ItemKind::ShearsCopper
            | ItemKind::ShovelCopper
            | ItemKind::SickleCopper
            | ItemKind::BeetrootSeeds
            | ItemKind::Beetroot
            | ItemKind::BeetrootRoasted
            | ItemKind::Cherry
            | ItemKind::Coal
            | ItemKind::Charcoal => ItemClass::Misc,
            ItemKind::HelmetChainmail => ItemClass::Armor(BodySlot::Head),
            ItemKind::ChestplateChainmail => ItemClass::Armor(BodySlot::Chest),
            ItemKind::LeggingsChainmail => ItemClass::Armor(BodySlot::Legs),
            ItemKind::BootsChainmail => ItemClass::Armor(BodySlot::Boots),
        }
    }

    /// Максимальный размер стака предмета. Инструменты и оружие не
    /// стакаются (по одному в слоте; в будущем у каждого будет своё
    /// состояние/износ), расходники и материалы — до `MAX_STACK`.
    /// Match без wildcard: новый вид не компилируется, пока предел не решён.
    pub fn max_stack(self) -> u32 {
        match self {
            ItemKind::AxePrimal
            | ItemKind::PickaxePrimal
            | ItemKind::HoePrimal
            | ItemKind::ShovelPrimal
            | ItemKind::SpearPrimal
            | ItemKind::KnifePrimal
            | ItemKind::FishingRod
            | ItemKind::Bow
            | ItemKind::HelmetChainmail
            | ItemKind::ChestplateChainmail
            | ItemKind::LeggingsChainmail
            | ItemKind::BootsChainmail
            | ItemKind::HammerStone
            | ItemKind::AxeCopper
            | ItemKind::HammerCopper
            | ItemKind::HoeCopper
            | ItemKind::PickaxeCopper
            | ItemKind::SawCopper
            | ItemKind::ShearsCopper
            | ItemKind::ShovelCopper
            | ItemKind::SickleCopper => 1,
            ItemKind::Stick
            | ItemKind::Rock
            | ItemKind::Apple
            | ItemKind::GrassBunch
            | ItemKind::PlantFiber
            | ItemKind::Cordage
            | ItemKind::Arrow
            | ItemKind::Bone
            | ItemKind::ShardBone
            | ItemKind::ArrowFletching
            | ItemKind::Feather
            | ItemKind::String
            | ItemKind::StickBundle
            | ItemKind::Book
            | ItemKind::Paper
            | ItemKind::PaperPulp
            | ItemKind::ClayClod
            | ItemKind::ClayBrickUnfired
            | ItemKind::ClayBrickBaked
            | ItemKind::WheatSeeds
            | ItemKind::Wheat
            | ItemKind::NuggetCopper
            | ItemKind::NuggetGold
            | ItemKind::NuggetSilver
            | ItemKind::IngotCopper
            | ItemKind::IngotBronze
            | ItemKind::IngotTin
            | ItemKind::IngotGold
            | ItemKind::IngotSilver
            | ItemKind::GemQuartz
            | ItemKind::GemFlint
            | ItemKind::GemEmerald
            | ItemKind::GemRuby
            | ItemKind::DustBronze
            | ItemKind::DustCharcoal
            | ItemKind::DustCopper
            | ItemKind::DustFlour
            | ItemKind::DustGold
            | ItemKind::DustPitAsh
            | ItemKind::DustSilver
            | ItemKind::DustSugar
            | ItemKind::DustSulfur
            | ItemKind::DustTin
            | ItemKind::DustWoodChips
            | ItemKind::AppleBaked
            | ItemKind::Carrot
            | ItemKind::CarrotBaked
            | ItemKind::Egg
            | ItemKind::EggRoasted
            | ItemKind::Lard
            | ItemKind::MeatBirdRaw
            | ItemKind::MeatBirdCooked
            | ItemKind::MeatHerbivoreRaw
            | ItemKind::MeatHerbivoreCooked
            | ItemKind::MeatPredatorRaw
            | ItemKind::MeatPredatorCooked
            | ItemKind::Potato
            | ItemKind::PotatoBaked
            | ItemKind::StrangeTuber
            | ItemKind::RawCopper
            | ItemKind::RawGold
            | ItemKind::RawSilver
            | ItemKind::RawTin
            | ItemKind::BeetrootSeeds
            | ItemKind::Beetroot
            | ItemKind::BeetrootRoasted
            | ItemKind::Cherry
            | ItemKind::Coal
            | ItemKind::Charcoal => MAX_STACK,
        }
    }

    /// Характеристики ближнего боя предмета; `None` — предмет не оружие.
    /// Атака без оружия (или этим предметом) — базовый удар рукой
    /// (см. `HAND_ATTACK`). Копьё в ближнем бою — укол: +20% к базовой
    /// дальности, топор — +5%.
    pub fn weapon_stats(self) -> Option<WeaponStats> {
        match self {
            ItemKind::KnifePrimal => Some(WeaponStats {
                damage: 5.0,
                cooldown_ticks: 6,
                range_tiles: BASE_ATTACK_RANGE_TILES,
            }),
            ItemKind::SpearPrimal => Some(WeaponStats {
                damage: 5.0,
                cooldown_ticks: 10,
                range_tiles: BASE_ATTACK_RANGE_TILES * 1.2,
            }),
            ItemKind::AxePrimal => Some(WeaponStats {
                damage: 7.0,
                cooldown_ticks: 15,
                range_tiles: BASE_ATTACK_RANGE_TILES * 1.05,
            }),
            // Лук — оружие дальнего боя: в ближнем бою бесполезен (стрельба
            // обрабатывается отдельно), рука бьёт базовым ударом.
            _ => None,
        }
    }

    /// Класс инструмента/оружия: обобщённая проверка «умеет ли предмет
    /// это действие» (любая кирка добывает камень, любая мотыга пашет —
    /// идея из Minecraft). `None` — предмет не инструмент. Match без
    /// wildcard: новый инструмент не компилируется, пока класс не решён.
    pub fn tool_class(self) -> Option<ToolClass> {
        match self {
            ItemKind::PickaxePrimal | ItemKind::PickaxeCopper => Some(ToolClass::Pickaxe),
            ItemKind::AxePrimal | ItemKind::AxeCopper => Some(ToolClass::Axe),
            ItemKind::ShovelPrimal | ItemKind::ShovelCopper => Some(ToolClass::Shovel),
            ItemKind::HoePrimal | ItemKind::HoeCopper => Some(ToolClass::Hoe),
            ItemKind::SpearPrimal => Some(ToolClass::Spear),
            ItemKind::KnifePrimal => Some(ToolClass::Knife),
            ItemKind::HammerStone | ItemKind::HammerCopper => Some(ToolClass::Hammer),
            ItemKind::SawCopper => Some(ToolClass::Saw),
            ItemKind::SickleCopper => Some(ToolClass::Sickle),
            ItemKind::ShearsCopper => Some(ToolClass::Shears),
            // Класс Sword зарезервирован: мечей в игре пока нет.
            _ => None,
        }
    }

    /// Время горения предмета как топлива в тиках печи; 0 — не топливо.
    /// Плавильная печь принимает только уголь (см. `containers.rs`).
    pub fn burn_time(self) -> u32 {
        match self {
            ItemKind::Stick => 100,
            ItemKind::StickBundle => 400,
            ItemKind::DustWoodChips => 50,
            ItemKind::DustCharcoal => 400,
            ItemKind::Charcoal => 1200,
            ItemKind::Coal => 1600,
            _ => 0,
        }
    }

    /// Питательность еды в единицах сытости (`Hunger::eat`); 0 —
    /// несъедобное. Значения согласованны: сырая еда дешевле готовой,
    /// ягоды — лёгкая еда, мясо — сытная.
    pub fn food_value(self) -> f32 {
        match self {
            ItemKind::Apple => 10.0,
            ItemKind::AppleBaked => 14.0,
            ItemKind::Cherry => 3.0,
            ItemKind::Carrot => 4.0,
            ItemKind::CarrotBaked => 7.0,
            ItemKind::Potato => 4.0,
            ItemKind::PotatoBaked => 8.0,
            ItemKind::Beetroot => 4.0,
            ItemKind::BeetrootRoasted => 7.0,
            ItemKind::Egg => 4.0,
            ItemKind::EggRoasted => 7.0,
            ItemKind::WheatSeeds => 2.0,
            ItemKind::Lard => 10.0,
            ItemKind::StrangeTuber => 6.0,
            ItemKind::MeatBirdRaw => 6.0,
            ItemKind::MeatBirdCooked => 12.0,
            ItemKind::MeatHerbivoreRaw => 7.0,
            ItemKind::MeatHerbivoreCooked => 14.0,
            ItemKind::MeatPredatorRaw => 5.0,
            ItemKind::MeatPredatorCooked => 10.0,
            _ => 0.0,
        }
    }

    /// Еда ли это для волка: хищник ест мясо (сырое и приготовленное).
    /// Остальная еда и несъедобное — нет; новый вид должен решить это
    /// явно (match без wildcard, как у `food_value`).
    pub fn is_wolf_food(self) -> bool {
        matches!(
            self,
            ItemKind::MeatBirdRaw
                | ItemKind::MeatBirdCooked
                | ItemKind::MeatHerbivoreRaw
                | ItemKind::MeatHerbivoreCooked
                | ItemKind::MeatPredatorRaw
                | ItemKind::MeatPredatorCooked
        )
    }
}

/// Класс инструмента/оружия — обобщённое поведение вместо проверки десятков
/// отдельных видов (любая кирка добывает камень, любая мотыга пашет землю).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)] // Sword — задел: мечей в игре пока нет
pub enum ToolClass {
    Pickaxe,
    Axe,
    Shovel,
    Hoe,
    Sword,
    Knife,
    Hammer,
    Spear,
    Saw,
    Sickle,
    Shears,
}

/// Базовый радиус атаки в тайлах — расстояние от существа до цели.
/// Совпадает с радиусом взаимодействия игрока (`INTERACT_REACH`).
pub const BASE_ATTACK_RANGE_TILES: f32 = 2.0;

/// Базовая атака: рука или любой предмет без собственных характеристик.
pub const HAND_ATTACK: WeaponStats = WeaponStats {
    damage: 1.0,
    cooldown_ticks: 10,
    range_tiles: BASE_ATTACK_RANGE_TILES,
};

/// Характеристики удара в ближнем бою.
#[derive(Clone, Copy, Debug)]
pub struct WeaponStats {
    /// Урон за удар (HP существа).
    pub damage: f32,
    /// Кулдаун в тиках: следующая атака не раньше этого смещения.
    pub cooldown_ticks: u64,
    /// Радиус атаки в тайлах: цель не дальше этого расстояния от атакующего.
    pub range_tiles: f32,
}
