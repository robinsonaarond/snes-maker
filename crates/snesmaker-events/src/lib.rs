use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DialogueGraph {
    pub id: String,
    pub opening_node: String,
    pub nodes: Vec<DialogueNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DialogueNode {
    pub id: String,
    pub speaker: String,
    pub text: String,
    #[serde(default)]
    pub commands: Vec<EventCommand>,
    #[serde(default)]
    pub choices: Vec<DialogueChoice>,
    pub next: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DialogueChoice {
    pub text: String,
    pub next: String,
    pub condition_flag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventScript {
    pub id: String,
    #[serde(default)]
    pub commands: Vec<EventCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TriggerKind {
    Touch,
    Interact,
    EnterScene,
    DefeatAllEnemies,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EventCommand {
    ShowDialogue {
        dialogue_id: String,
        node_id: Option<String>,
    },
    SetFlag {
        flag: String,
        value: bool,
    },
    Wait {
        frames: u16,
    },
    MoveCamera {
        target_x: i16,
        target_y: i16,
        frames: u16,
    },
    FreezePlayer {
        frozen: bool,
    },
    SpawnEntity {
        archetype: String,
        x: i16,
        y: i16,
    },
    LoadScene {
        scene_id: String,
        spawn: Option<String>,
    },
    StartBattleScene {
        battle_id: String,
    },
    PlayCutscene {
        cutscene_id: String,
    },
    EmitCheckpoint {
        checkpoint_id: String,
    },
}

pub fn reserved_unimplemented_commands(script: &EventScript) -> Vec<&'static str> {
    script
        .commands
        .iter()
        .filter_map(|command| match command {
            EventCommand::StartBattleScene { .. } => Some("StartBattleScene"),
            _ => None,
        })
        .collect()
}
