//! Предметы и инвентарь (GAME_DESIGN 3.7).
//! Типы предметов — данные со стабильными идентификаторами; инвентарь —
//! стеки: слот = предмет одного вида + количество. Слоты разделены
//! по типам: misc (любой предмет), броня и одежда (по частям тела),
//! косметика.

use super::tile::ResourceKind;

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
    Axe,
    /// Примитивная кирка.
    Pickaxe,
    /// Примитивная мотыга.
    Hoe,
    /// Примитивная лопата.
    Shovel,
    /// Примитивное копьё.
    Spear,
    /// Каменный нож.
    Knife,
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
        ItemKind::Axe,
        ItemKind::Pickaxe,
        ItemKind::Hoe,
        ItemKind::Shovel,
        ItemKind::Spear,
        ItemKind::Knife,
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
            6 => Some(Self::Axe),
            7 => Some(Self::Pickaxe),
            8 => Some(Self::Hoe),
            9 => Some(Self::Shovel),
            10 => Some(Self::Spear),
            11 => Some(Self::Knife),
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
            ItemKind::Axe => "axe",
            ItemKind::Pickaxe => "pickaxe",
            ItemKind::Hoe => "hoe",
            ItemKind::Shovel => "shovel",
            ItemKind::Spear => "spear",
            ItemKind::Knife => "knife",
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
            | ItemKind::Axe
            | ItemKind::Pickaxe
            | ItemKind::Hoe
            | ItemKind::Shovel
            | ItemKind::Spear
            | ItemKind::Knife
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
            ItemKind::Axe
            | ItemKind::Pickaxe
            | ItemKind::Hoe
            | ItemKind::Shovel
            | ItemKind::Spear
            | ItemKind::Knife
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
            ItemKind::Knife => Some(WeaponStats {
                damage: 5.0,
                cooldown_ticks: 6,
                range_tiles: BASE_ATTACK_RANGE_TILES,
            }),
            ItemKind::Spear => Some(WeaponStats {
                damage: 5.0,
                cooldown_ticks: 10,
                range_tiles: BASE_ATTACK_RANGE_TILES * 1.2,
            }),
            ItemKind::Axe => Some(WeaponStats {
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
            ItemKind::Pickaxe | ItemKind::PickaxeCopper => Some(ToolClass::Pickaxe),
            ItemKind::Axe | ItemKind::AxeCopper => Some(ToolClass::Axe),
            ItemKind::Shovel | ItemKind::ShovelCopper => Some(ToolClass::Shovel),
            ItemKind::Hoe | ItemKind::HoeCopper => Some(ToolClass::Hoe),
            ItemKind::Spear => Some(ToolClass::Spear),
            ItemKind::Knife => Some(ToolClass::Knife),
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

/// Класс предмета — правило размещения по слотам.
#[allow(dead_code)] // Clothes и Bauble появятся вместе с первыми предметами этих классов
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemClass {
    /// Обычный предмет: лежит в основных слотах `misc`.
    Misc,
    /// Броня конкретной части тела: свой слот брони (misc тоже принимает —
    /// «в основном слоте может лежать что угодно»).
    Armor(BodySlot),
    /// Одежда конкретной части тела: свой слот одежды (misc тоже
    /// принимает). Защита от среды (холод и тому подобное), не карман.
    Clothes(BodySlot),
    /// Косметика: любой из слотов косметики (misc тоже принимает).
    Bauble,
}

/// Часть тела — специализация слота и предмета снаряжения (броня, одежда).
/// Порядок первых четырёх — порядок слотов брони и одежды разумного
/// существа (GAME_DESIGN 3.4: head, chest, legs, boots); `Body` — единая
/// специализация на всё тело для неразумных существ.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum BodySlot {
    Head,
    Chest,
    Legs,
    Boots,
    /// Неразумные существа: единое снаряжение на тело (без деления по частям).
    Body,
}

/// Максимум единиц одного предмета в слоте.
pub const MAX_STACK: u32 = 64;

/// Число слотов предметов (misc) разумного существа (GAME_DESIGN 3.4: 8).
pub const MISC_SLOTS: usize = 8;
/// Число слотов брони разумного существа (GAME_DESIGN 3.4: 4).
pub const ARMOR_SLOTS: usize = 4;
/// Число слотов одежды разумного существа: по части тела, как у брони.
pub const CLOTHES_SLOTS: usize = 4;
/// Число слотов косметики разумного существа (GAME_DESIGN 3.4: 4).
pub const BAUBLE_SLOTS: usize = 4;

/// Части тела слотов брони разумного существа — порядок фиксирован:
/// head, chest, legs, boots. Индекс в секции брони = индекс в этом списке.
pub const ARMOR_LAYOUT: [BodySlot; ARMOR_SLOTS] =
    [BodySlot::Head, BodySlot::Chest, BodySlot::Legs, BodySlot::Boots];

/// Части тела слотов одежды разумного существа — совпадает с `ARMOR_LAYOUT`
/// (одежда делится по тем же частям тела, что и броня).
pub const CLOTHES_LAYOUT: [BodySlot; CLOTHES_SLOTS] =
    [BodySlot::Head, BodySlot::Chest, BodySlot::Legs, BodySlot::Boots];

/// Специализации слотов брони неразумного существа (GAME_DESIGN 3.4:
/// «1 слот инвентаря + 2 слота особой брони»: head и body). Задел на
/// будущих существ; инвентарь игрока пока разумный.
#[allow(dead_code)] // задел до первого неразумного существа
pub const ARMOR_LAYOUT_UNSENTIENT: [BodySlot; 2] = [BodySlot::Head, BodySlot::Body];

/// Секция инвентаря — глобальная типизация слотов.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotArea {
    /// Основные предметы (misc): любой предмет может лежать в любом слоте.
    Misc,
    /// Броня: слот принимает только броню своей части тела.
    Armor,
    /// Одежда: слот принимает только одежду своей части тела.
    Clothes,
    /// Косметика (baubles): слоты равнозначны, порядок произвольный.
    Bauble,
    /// Мусорка UI удаления предметов: один слот, принимает что угодно,
    /// каждая новая кладка затирает старое (стек можно забрать обратно,
    /// пока его не затёрли). Не входит в `ALL`: предмет в мусорке не
    /// считается лежащим в инвентаре (не тратится, не съедается).
    Trash,
}

impl SlotArea {
    /// Секции в стабильном порядке (UI, обходы). Мусорка — служебный
    /// слот удаления, в обычные обходы инвентаря не входит.
    pub const ALL: [SlotArea; 4] =
        [SlotArea::Misc, SlotArea::Armor, SlotArea::Clothes, SlotArea::Bauble];

    /// Число слотов секции у разумного существа (GAME_DESIGN 3.4).
    pub fn slots_count(self) -> usize {
        match self {
            SlotArea::Misc => MISC_SLOTS,
            SlotArea::Armor => ARMOR_SLOTS,
            SlotArea::Clothes => CLOTHES_SLOTS,
            SlotArea::Bauble => BAUBLE_SLOTS,
            SlotArea::Trash => 1,
        }
    }
}

/// Адрес конкретного слота инвентаря: секция + индекс в ней.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SlotRef {
    pub area: SlotArea,
    pub index: usize,
}

/// Стек предметов одного вида в слоте.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ItemStack {
    pub kind: ItemKind,
    pub count: u32,
}

/// Инвентарь: секции слотов (misc, броня, одежда, косметика).
/// Слот пуст (None) или хранит стек предметов одного вида.
/// Секции брони и одежды разумного существа индексируются по
/// `ARMOR_LAYOUT`/`CLOTHES_LAYOUT`.
/// Для неразумных существ (ARMOR_LAYOUT_UNSENTIENT) отдельный инвентарь.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Inventory {
    /// Основные предметы.
    pub items: [Option<ItemStack>; MISC_SLOTS],
    /// Надеваемая броня (не используется на первом срезе).
    pub armor: [Option<ItemStack>; ARMOR_SLOTS],
    /// Надеваемая одежда: защита от среды (холод и т.п.), предметов пока нет.
    pub clothes: [Option<ItemStack>; CLOTHES_SLOTS],
    /// Косметика (не используется на первом срезе).
    pub baubles: [Option<ItemStack>; BAUBLE_SLOTS],
    /// Мусорка UI: последний положенный «на удаление» стек. Затирается
    /// следующей кладкой, поэтому хранит ровно один слот.
    pub trash: Option<ItemStack>,
}

impl Default for Inventory {
    /// Пустой инвентарь: все слоты свободны.
    fn default() -> Self {
        Self {
            items: [None; MISC_SLOTS],
            armor: [None; ARMOR_SLOTS],
            clothes: [None; CLOTHES_SLOTS],
            baubles: [None; BAUBLE_SLOTS],
            trash: None,
        }
    }
}

impl Inventory {
    /// Добавляет предметы в секцию misc: доливает в существующий стек
    /// того же вида или занимает пустые слоты. Предел стака — по виду
    /// (`ItemKind::max_stack`): инструменты и оружие расходуют по слоту
    /// на единицу, расходники доливаются до 64. Возвращает, сколько
    /// единиц не поместилось.
    pub fn add(&mut self, kind: ItemKind, count: u32) -> u32 {
        let cap = kind.max_stack();
        let mut left = count;

        // Сначала доливаем частично заполненные стеки того же вида.
        for slot in self.items.iter_mut().flatten() {
            if left == 0 {
                break;
            }
            if slot.kind == kind && slot.count < cap {
                let take = (cap - slot.count).min(left);
                slot.count += take;
                left -= take;
            }
        }

        // Остаток занимает пустые слоты — тоже с пределом вида.
        for slot in self.items.iter_mut() {
            if left == 0 {
                break;
            }
            if slot.is_none() {
                let take = cap.min(left);
                *slot = Some(ItemStack { kind, count: take });
                left -= take;
            }
        }

        left
    }

    /// Поместятся ли `count` единиц предмета вида `kind` в секцию misc
    /// целиком (предел стака — по виду). Используется перед действиями,
    /// которые тратят ресурс мира: добыча «в никуда» не должна
    /// уничтожать объект.
    pub fn can_fit(&self, kind: ItemKind, count: u32) -> bool {
        let cap = kind.max_stack();
        let mut space = 0_u32;
        for slot in &self.items {
            match slot {
                Some(stack) if stack.kind == kind => space += cap - stack.count,
                None => space += cap,
                Some(_) => {}
            }
        }
        space >= count
    }

    /// Сколько единиц предмета вида `kind` лежит в инвентаре (все секции,
    /// хотя на первом срезе добыча кладёт предмет только в `items`).
    pub fn count_of(&self, kind: ItemKind) -> u32 {
        let mut total = 0_u32;
        for area in SlotArea::ALL {
            for slot in self.area(area).iter().flatten() {
                if slot.kind == kind {
                    total += slot.count;
                }
            }
        }
        total
    }

    /// Списывает `count` единиц предмета `kind` из секции misc (по стекам,
    /// начиная с первого слота). `false` — предметов не хватает, тогда
    /// ничего не списано. Нужно строительству: плата берётся со всех
    /// стеков инвентаря, а не только с выбранного слота.
    pub fn take_items(&mut self, kind: ItemKind, count: u32) -> bool {
        let mut available = 0_u32;
        for slot in self.items.iter().flatten() {
            if slot.kind == kind {
                available += slot.count;
            }
        }
        if available < count {
            return false;
        }
        let mut left = count;
        for slot in self.items.iter_mut() {
            if left == 0 {
                break;
            }
            let Some(stack) = slot else { continue };
            if stack.kind != kind {
                continue;
            }
            let take = stack.count.min(left);
            stack.count -= take;
            left -= take;
            if stack.count == 0 {
                *slot = None;
            }
        }
        true
    }

    /// Слоты секции (view для UI и обходов).
    pub fn area(&self, area: SlotArea) -> &[Option<ItemStack>] {
        match area {
            SlotArea::Misc => &self.items,
            SlotArea::Armor => &self.armor,
            SlotArea::Clothes => &self.clothes,
            SlotArea::Bauble => &self.baubles,
            SlotArea::Trash => std::slice::from_ref(&self.trash),
        }
    }

    /// Мутабельные слоты секции.
    pub fn area_mut(&mut self, area: SlotArea) -> &mut [Option<ItemStack>] {
        match area {
            SlotArea::Misc => &mut self.items,
            SlotArea::Armor => &mut self.armor,
            SlotArea::Clothes => &mut self.clothes,
            SlotArea::Bauble => &mut self.baubles,
            SlotArea::Trash => std::slice::from_mut(&mut self.trash),
        }
    }

    /// Стек слота (нечитаемый адрес — None).
    pub fn get(&self, slot: SlotRef) -> Option<ItemStack> {
        self.area(slot.area).get(slot.index).copied().flatten()
    }

    /// Берёт стек из слота, очищая его. Нечитаемый адрес — None.
    pub fn take(&mut self, slot: SlotRef) -> Option<ItemStack> {
        self.area_mut(slot.area).get_mut(slot.index).and_then(Option::take)
    }

    /// Ставит стек в пустой слот. Нечитаемый адрес — стек теряется:
    /// вызывающий обязан проверить совместимость через `accepts` заранее
    /// (внутренний метод, правила размещения — в `accepts` и `transfer`).
    pub fn put(&mut self, slot: SlotRef, stack: ItemStack) {
        if let Some(cell) = self.area_mut(slot.area).get_mut(slot.index) {
            debug_assert!(cell.is_none(), "слот занят: запись только в пустой");
            *cell = Some(stack);
        }
    }

    /// Правило размещения: можно ли предмет вида `kind` держать в слоте
    /// `area[index]`. Misc принимает любой предмет; свой слот брони — только
    /// броню той же части тела; одежды — только одежду той же части тела;
    /// косметику — только косметика; мусорка — что угодно (старый стек
    /// затирается).
    pub fn accepts(kind: ItemKind, area: SlotArea, index: usize) -> bool {
        if index >= area.slots_count() {
            return false;
        }
        match area {
            SlotArea::Misc => true,
            SlotArea::Armor => {
                matches!(kind.class(), ItemClass::Armor(slot) if slot == ARMOR_LAYOUT[index])
            }
            SlotArea::Clothes => {
                matches!(kind.class(), ItemClass::Clothes(slot) if slot == CLOTHES_LAYOUT[index])
            }
            SlotArea::Bauble => matches!(kind.class(), ItemClass::Bauble),
            SlotArea::Trash => true,
        }
    }

    /// Переносит содержимое слота в другой слот с учётом правил размещения.
    /// Целевой слот занят тем же видом (misc-стеки) — доливает, остаток
    /// возвращается в исходный слот; занят другим предметом — меняет их
    /// местами, если встречный предмет допустим в исходном слоте.
    /// Целевой слот — мусорка: принимает что угодно, старый стек затирается
    /// (предмет удаляется — это и есть путь удаления через UI). Исключение
    /// до общей логики: ни слияния, ни обмена с мусоркой не бывает.
    /// `true` — содержимое изменилось.
    pub fn transfer(&mut self, from: SlotRef, to: SlotRef) -> bool {
        if from == to || !self.accepts_slot_count(to) {
            return false;
        }
        let Some(source) = self.take(from) else {
            return false;
        };
        if to.area == SlotArea::Trash {
            self.take(to); // прежнее содержимое удаляется
            self.put(to, source);
            return true;
        }
        if !Self::accepts(source.kind, to.area, to.index) {
            self.put(from, source);
            return false;
        }

        let target = self.take(to);
        match target {
            None => {
                self.put(to, source);
                true
            }
            Some(t) if t.kind == source.kind => {
                // Доливаем целевой стек до предела вида, остаток остаётся
                // в исходном слоте.
                let cap = t.kind.max_stack();
                let free = cap.saturating_sub(t.count).min(source.count);
                if free > 0 {
                    self.put(to, ItemStack { kind: t.kind, count: t.count + free });
                    if source.count > free {
                        self.put(from, ItemStack { kind: t.kind, count: source.count - free });
                    }
                    true
                } else {
                    // Некуда доливать: перенос невозможен, всё на месте.
                    self.put(to, t);
                    self.put(from, source);
                    false
                }
            }
            Some(t) => {
                if Self::accepts(t.kind, from.area, from.index) {
                    // Обмен предметами между слотами.
                    self.put(to, source);
                    self.put(from, t);
                    true
                } else {
                    self.put(to, t);
                    self.put(from, source);
                    false
                }
            }
        }
    }

    /// Адрес в границах секции.
    fn accepts_slot_count(&self, slot: SlotRef) -> bool {
        slot.index < slot.area.slots_count()
    }
}
