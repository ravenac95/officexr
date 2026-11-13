//! iOS has no flags or env vars to start from, so the config comes from an in-app screen and is
//! saved in the app's Documents directory.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::text::{EditableText, TextCursorStyle};
use bevy::ui_widgets::{Activate, Button, TextInput};

use crate::config::Config;

const FIELDS: [&str; 4] = ["Web URL", "Realtime URL", "Office id", "Session cookie"];

fn saved_path() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Documents/officexr-config.json")
}

/// Flags or env vars (e.g. from an Xcode scheme), else the saved config, else the config screen.
pub fn load_config() -> Config {
    Config::from_env_and_args()
        .ok()
        .or_else(|| serde_json::from_slice(&std::fs::read(saved_path()).ok()?).ok())
        .unwrap_or_else(|| config_screen(None, None))
}

pub fn connect_failed(config: &Config, err: String) -> ! {
    error!("{err}");
    config_screen(Some(config), Some(err))
}

#[derive(Resource)]
struct Form {
    values: [String; 4],
    error: Option<String>,
}

#[derive(Component)]
struct Field(usize);

// ponytail: saving quits the app and the next launch joins; there is no way back to this screen
// from a room that joined.
fn config_screen(config: Option<&Config>, error: Option<String>) -> ! {
    let values = config.map_or_else(Default::default, |c| {
        [
            c.web_url.clone(),
            c.realtime_url.clone(),
            c.office_id.clone(),
            c.session_cookie.clone(),
        ]
    });
    App::new()
        .add_plugins(DefaultPlugins)
        .insert_resource(Form { values, error })
        .add_systems(Startup, setup)
        .run();
    std::process::exit(0);
}

fn setup(mut commands: Commands, form: Res<Form>) {
    commands.spawn(Camera2d);
    commands
        .spawn(Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            padding: px(24).all(),
            row_gap: px(8),
            ..default()
        })
        .with_children(|parent| {
            parent.spawn(Text::new("OfficeXR"));
            if let Some(error) = &form.error {
                parent.spawn((
                    Text::new(error.clone()),
                    TextColor(Color::srgb(1.0, 0.4, 0.4)),
                ));
            }
            for (i, label) in FIELDS.into_iter().enumerate() {
                parent.spawn(Text::new(label));
                parent.spawn((
                    Node {
                        padding: px(6).all(),
                        ..default()
                    },
                    TextInput,
                    EditableText::new(&form.values[i]),
                    TextCursorStyle::default(),
                    TextLayout::no_wrap(),
                    BackgroundColor(Color::srgb(0.2, 0.2, 0.25)),
                    Field(i),
                ));
            }
            parent
                .spawn((
                    Node {
                        padding: px(10).all(),
                        margin: px(8).top(),
                        ..default()
                    },
                    Button,
                    BackgroundColor(Color::srgb(0.2, 0.4, 0.8)),
                    children![Text::new("Save and quit; reopen to join")],
                ))
                .observe(save);
        });
}

fn save(_: On<Activate>, fields: Query<(&Field, &EditableText)>, mut exit: MessageWriter<AppExit>) {
    let mut values: [String; 4] = Default::default();
    for (field, text) in &fields {
        values[field.0] = text.value().to_string().trim().to_string();
    }
    let [web_url, realtime_url, office_id, session_cookie] = values;
    let config = Config {
        web_url: web_url.trim_end_matches('/').into(),
        realtime_url,
        office_id,
        session_cookie,
    };
    let json = serde_json::to_vec(&config).expect("config serializes");
    if let Err(err) = std::fs::write(saved_path(), json) {
        error!("saving the config: {err}");
        return;
    }
    exit.write(AppExit::Success);
}
