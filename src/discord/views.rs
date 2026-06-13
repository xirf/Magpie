use serenity::prelude::*;
use serenity::model::application::{Interaction, ButtonStyle};
use serenity::builder::{
    CreateInteractionResponse, CreateInteractionResponseMessage,
    CreateActionRow, CreateButton, CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption
};
use crate::config::{Settings, UserSettings};
use crate::utils::{convert_size, convert_eta, format_progress};
use super::state::{Handler, translate};

pub async fn show_torrent_list(
    handler: &Handler,
    ctx: &Context,
    interaction: &Interaction,
    user: &UserSettings,
    status_filter: Option<&str>,
) -> Result<(), String> {
    let clean_filter = match status_filter {
        Some("all") | None => None,
        Some(s) => Some(s),
    };

    let torrents = handler.manager.get_torrents(None, clean_filter).await?;

    // Status row buttons
    let dl_btn = CreateButton::new("dc_status:downloading")
        .label("⏳ Downloading")
        .style(if clean_filter == Some("downloading") { ButtonStyle::Primary } else { ButtonStyle::Secondary });
    
    let comp_btn = CreateButton::new("dc_status:completed")
        .label("✔️ Completed")
        .style(if clean_filter == Some("completed") { ButtonStyle::Primary } else { ButtonStyle::Secondary });

    let pause_btn = CreateButton::new("dc_status:paused")
        .label("⏸️ Paused")
        .style(if clean_filter == Some("paused") { ButtonStyle::Primary } else { ButtonStyle::Secondary });

    let all_btn = CreateButton::new("dc_status:all")
        .label("📁 All")
        .style(if clean_filter.is_none() { ButtonStyle::Primary } else { ButtonStyle::Secondary });

    let filter_row = CreateActionRow::Buttons(vec![all_btn, dl_btn, comp_btn, pause_btn]);

    if torrents.is_empty() {
        let filter_name = clean_filter.map(|s| {
            let mut chars = s.chars();
            match chars.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + chars.as_str()
            }
        }).unwrap_or_else(|| "All".to_string());

        let content = if clean_filter.is_some() {
            format!("No torrents found with status: **{}**", filter_name)
        } else {
            translate(user, "There are no torrents", None)
        };

        let components = if clean_filter.is_some() { vec![filter_row] } else { vec![] };

        match interaction {
            Interaction::Component(comp) => {
                comp.create_response(&ctx.http, CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new().content(content).embeds(vec![]).components(components)
                )).await.map_err(|e| e.to_string())?;
            }
            Interaction::Command(cmd) => {
                cmd.create_response(&ctx.http, CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new().content(content).components(components)
                )).await.map_err(|e| e.to_string())?;
            }
            _ => {}
        }
        return Ok(());
    }

    let mut desc = String::new();
    for t in &torrents {
        let state = {
            let mut chars = t.state.chars();
            match chars.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + chars.as_str()
            }
        };
        let progress_percent = (t.progress * 100.0).round() as i32;
        let size_str = convert_size(t.size);
        let dl_speed_str = convert_size(t.dlspeed);

        desc.push_str(&format!("**{}**\n", t.name));
        desc.push_str(&format!("{} {}%\n", format_progress(t.progress, 20), progress_percent));
        desc.push_str(&format!("State: `{}` | Size: `{}` | Speed: `{}/s`\n", state, size_str, dl_speed_str));
        desc.push_str(&format!("Hash: `{}`\n\n", t.hash));
    }

    if desc.len() > 4096 {
        desc.truncate(4093);
        desc.push_str("...");
    }

    let embed = serenity::builder::CreateEmbed::new()
        .title(translate(user, "Welcome to QBittorrent Bot", None))
        .color(0x00ae86)
        .description(desc);

    // Select menu for torrents
    let mut select_options = Vec::new();
    for t in torrents.iter().take(25) {
        let label = if t.name.len() > 100 { &t.name[..97] } else { &t.name };
        let desc_str = format!("Size: {} | Status: {}", convert_size(t.size), t.state);
        let desc_slice = if desc_str.len() > 100 { &desc_str[..97] } else { &desc_str };
        select_options.push(CreateSelectMenuOption::new(label, &t.hash).description(desc_slice));
    }

    let select_menu = CreateSelectMenu::new("select_torrent", CreateSelectMenuKind::String {
        options: select_options,
    }).placeholder("Select a torrent to manage...");

    let select_row = CreateActionRow::SelectMenu(select_menu);

    let components = vec![select_row, filter_row];

    match interaction {
        Interaction::Component(comp) => {
            comp.create_response(&ctx.http, CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new().content("").embed(embed).components(components)
            )).await.map_err(|e| e.to_string())?;
        }
        Interaction::Command(cmd) => {
            cmd.create_response(&ctx.http, CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().content("").embed(embed).components(components)
            )).await.map_err(|e| e.to_string())?;
        }
        _ => {}
    }

    Ok(())
}

pub async fn show_torrent_details(
    handler: &Handler,
    ctx: &Context,
    interaction: &Interaction,
    user: &UserSettings,
    hash: &str,
    settings: &Settings,
) -> Result<(), String> {
    let torrent = match handler.manager.get_torrent(hash, None).await? {
        Some(t) => t,
        None => {
            let content = translate(user, "Torrent not found", None);
            match interaction {
                Interaction::Component(comp) => {
                    comp.create_response(&ctx.http, CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new().content(content).embeds(vec![]).components(vec![])
                    )).await.map_err(|e| e.to_string())?;
                }
                Interaction::Command(cmd) => {
                    cmd.create_response(&ctx.http, CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new().content(content)
                    )).await.map_err(|e| e.to_string())?;
                }
                _ => {}
            }
            return Ok(());
        }
    };

    let state = {
        let mut chars = torrent.state.chars();
        match chars.next() {
            None => String::new(),
            Some(f) => f.to_uppercase().collect::<String>() + chars.as_str()
        }
    };
    let progress_percent = (torrent.progress * 100.0).round() as i32;

    let embed = serenity::builder::CreateEmbed::new()
        .title(&torrent.name)
        .color(0x00ae86)
        .field("Progress", format!("{} {}%", format_progress(torrent.progress, 20), progress_percent), false)
        .field("State", format!("`{}`", state), true)
        .field("Size", format!("`{}`", convert_size(torrent.size)), true)
        .field("Download Speed", format!("`{}/s`", convert_size(torrent.dlspeed)), true)
        .field("ETA", format!("`{}`", convert_eta(torrent.eta)), true)
        .field("Category", format!("`{}`", torrent.category.as_deref().unwrap_or("None")), true)
        .field("Hash", format!("`{}`", torrent.hash), false);

    let pause_btn = CreateButton::new(format!("dc_pause:{}", torrent.hash))
        .label("Pause")
        .style(ButtonStyle::Secondary);

    let resume_btn = CreateButton::new(format!("dc_resume:{}", torrent.hash))
        .label("Resume")
        .style(ButtonStyle::Success);

    let delete_btn = CreateButton::new(format!("dc_delete:{}", torrent.hash))
        .label("Delete")
        .style(ButtonStyle::Danger);

    let back_btn = CreateButton::new("dc_back_list")
        .label("Back to List")
        .style(ButtonStyle::Secondary);

    let mut buttons = vec![pause_btn, resume_btn, delete_btn];
    if (settings.s3.enabled || settings.local_server.enabled) && torrent.progress >= 1.0 {
        let get_link_btn = CreateButton::new(format!("dc_get_link:{}", torrent.hash))
            .label("Get Link")
            .style(ButtonStyle::Primary);
        buttons.push(get_link_btn);
    }
    buttons.push(back_btn);

    let row = CreateActionRow::Buttons(buttons);

    match interaction {
        Interaction::Component(comp) => {
            comp.create_response(&ctx.http, CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new().content("").embed(embed).components(vec![row])
            )).await.map_err(|e| e.to_string())?;
        }
        Interaction::Command(cmd) => {
            cmd.create_response(&ctx.http, CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().content("").embed(embed).components(vec![row])
            )).await.map_err(|e| e.to_string())?;
        }
        _ => {}
    }

    Ok(())
}
