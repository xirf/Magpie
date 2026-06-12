use serenity::async_trait;
use serenity::model::application::{CommandOptionType, Interaction};
use serenity::builder::{
    CreateCommand, CreateCommandOption,
    CreateInteractionResponse, CreateInteractionResponseMessage,
    EditInteractionResponse, CreateActionRow, CreateButton, CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption
};
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::prelude::*;
use serenity::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::{Settings, UserSettings};
use crate::torrent_client::TorrentClient;
use crate::utils::{convert_size, convert_eta, format_progress, extract_hash_from_magnet, extract_clean_magnet};
use crate::i18n::t;

struct Handler {
    settings: Arc<RwLock<Settings>>,
    manager: Arc<dyn TorrentClient>,
}

fn translate(user: &UserSettings, key: &str, vars: Option<&HashMap<String, String>>) -> String {
    let locale = user.locale.as_deref().unwrap_or("en");
    t(key, locale, vars)
}

fn get_resolved_user(settings: &Settings, discord_id: &str) -> UserSettings {
    settings.users.iter()
        .find(|u| u.discord_id.as_deref() == Some(discord_id))
        .cloned()
        .or_else(|| {
            settings.users.iter()
                .find(|u| u.discord_id.is_none())
                .cloned()
        })
        .unwrap_or_else(|| {
            UserSettings {
                user_id: 0,
                discord_id: Some(discord_id.to_string()),
                role: "reader".to_string(),
                locale: Some("en".to_string()),
                notify: true,
                notification_filter: vec![],
            }
        })
}

impl Handler {
    async fn show_torrent_list(&self, ctx: &Context, interaction: &Interaction, user: &UserSettings, status_filter: Option<&str>) -> Result<(), String> {
        let clean_filter = match status_filter {
            Some("all") | None => None,
            Some(s) => Some(s),
        };

        let torrents = self.manager.get_torrents(None, clean_filter).await?;

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

    async fn show_torrent_details(&self, ctx: &Context, interaction: &Interaction, user: &UserSettings, hash: &str, settings: &Settings) -> Result<(), String> {
        let torrent = match self.manager.get_torrent(hash, None).await? {
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
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("Discord bot is online! Logged in as {}", ready.user.name);

        let guilds = &ready.guilds;

        let list_cmd = CreateCommand::new("list").description("List active torrents and manage them");
        let stats_cmd = CreateCommand::new("stats").description("Get host system statistics");
        let speedlimit_cmd = CreateCommand::new("speedlimit").description("Toggle alternate speed limits mode");
        let seeding_cmd = CreateCommand::new("seeding")
            .description("Set seeding policy after download completes (Admin only)")
            .add_option(
                CreateCommandOption::new(CommandOptionType::String, "policy", "Seeding policy")
                    .required(false)
                    .add_string_choice("Always", "always")
                    .add_string_choice("Never", "never")
                    .add_string_choice("Admin Only", "admin_only")
            );
        let add_cmd = CreateCommand::new("add")
            .description("Add a magnet link or torrent file URL")
            .add_option(
                CreateCommandOption::new(CommandOptionType::String, "link", "Magnet link or torrent file URL")
                    .required(true)
            );
        let info_cmd = CreateCommand::new("info")
            .description("Get details of a torrent by hash or message ID")
            .add_option(
                CreateCommandOption::new(CommandOptionType::String, "identifier", "Torrent Hash or Message ID")
                    .required(true)
            );
        let link_cmd = CreateCommand::new("link")
            .description("Get S3 download link of a completed torrent by hash or message ID")
            .add_option(
                CreateCommandOption::new(CommandOptionType::String, "identifier", "Torrent Hash or Message ID")
                    .required(true)
            );
        let info_menu = CreateCommand::new("Torrent Info")
            .kind(serenity::model::application::CommandType::Message);
        let link_menu = CreateCommand::new("Get Download Link")
            .kind(serenity::model::application::CommandType::Message);

        for guild in guilds {
            println!("Registering guild commands for guild ID: {}", guild.id);
            let _ = guild.id.set_commands(&ctx.http, vec![
                list_cmd.clone(),
                stats_cmd.clone(),
                speedlimit_cmd.clone(),
                seeding_cmd.clone(),
                add_cmd.clone(),
                info_cmd.clone(),
                link_cmd.clone(),
                info_menu.clone(),
                link_menu.clone(),
            ]).await;
        }
        println!("Discord slash commands registered successfully.");
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let discord_id = match &interaction {
            Interaction::Command(cmd) => cmd.user.id.to_string(),
            Interaction::Component(comp) => comp.user.id.to_string(),
            _ => return,
        };

        let settings = self.settings.read().await.clone();
        let mut user = get_resolved_user(&settings, &discord_id);

        // Auto auth first administrator
        let has_admin = settings.users.iter().any(|u| u.role == "administrator" && u.discord_id.as_deref().unwrap_or("") != "9876543210123");
        if !has_admin {
            user.role = "administrator".to_string();
            user.discord_id = Some(discord_id.clone());
            {
                let mut settings_write = self.settings.write().await;
                settings_write.users.push(user.clone());
                settings_write.export_settings();
                let _ = crate::db::save_user_to_db(&user);
            }
            if let Interaction::Command(ref cmd) = interaction {
                let _ = cmd.create_response(&ctx.http, CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new().content("👑 You have been automatically authorized as the first **administrator**!").ephemeral(true)
                )).await;
            }
            return;
        }

        // Check authorization
        let is_request_access = match &interaction {
            Interaction::Component(comp) => comp.data.custom_id == "dc_request_access",
            _ => false,
        };
        let is_auth_interaction = match &interaction {
            Interaction::Component(comp) => comp.data.custom_id.starts_with("dc_auth:"),
            _ => false,
        };

        if settings.users.iter().all(|u| u.discord_id.as_deref() != Some(&discord_id)) && !is_request_access && !is_auth_interaction {
            let row = CreateActionRow::Buttons(vec![
                CreateButton::new("dc_request_access").label("Request Access").style(ButtonStyle::Primary)
            ]);
            let content = "❌ You are not authorized to use this bot.";
            match interaction {
                Interaction::Command(cmd) => {
                    let _ = cmd.create_response(&ctx.http, CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new().content(content).components(vec![row]).ephemeral(true)
                    )).await;
                }
                Interaction::Component(comp) => {
                    let _ = comp.create_response(&ctx.http, CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new().content(content).components(vec![row]).ephemeral(true)
                    )).await;
                }
                _ => {}
            }
            return;
        }

        match &interaction {
            Interaction::Command(cmd) => {
                let name = cmd.data.name.as_str();
                match cmd.data.kind {
                    serenity::model::application::CommandType::ChatInput => {
                        match name {
                            "list" => {
                                if let Err(e) = self.show_torrent_list(&ctx, &interaction, &user, None).await {
                                    let _ = cmd.create_response(&ctx.http, CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new().content(format!("❌ Error: {}", e)).ephemeral(true)
                                    )).await;
                                }
                            }
                            "stats" => {
                                let _ = cmd.defer(&ctx.http).await;
                                // Get CPU temperature, usage, disk, memory via sysinfo
                                let mut sys = sysinfo::System::new_all();
                                sys.refresh_all();
                                
                                let cpu_usage = sys.global_cpu_info().cpu_usage().round() as i32;
                                
                                // Disk size
                                let mut disk_used = 0;
                                let mut disk_total = 0;
                                let mut disk_percent = 0;
                                let disks = sysinfo::Disks::new_with_refreshed_list();
                                if let Some(disk) = disks.iter().find(|d| d.mount_point().to_str() == Some("/mnt")).or_else(|| disks.iter().next()) {
                                    disk_total = disk.total_space();
                                    disk_used = disk_total - disk.available_space();
                                    if disk_total > 0 {
                                        disk_percent = ((disk_used as f64 / disk_total as f64) * 100.0).round() as i32;
                                    }
                                }

                                let total_mem = sys.total_memory();
                                let free_mem = sys.available_memory();
                                let mem_percent = if total_mem > 0 {
                                    (((total_mem - free_mem) as f64 / total_mem as f64) * 100.0).round() as i32
                                } else {
                                    0
                                };

                                let mut vars = HashMap::new();
                                vars.insert("cpu_usage".to_string(), cpu_usage.to_string());
                                vars.insert("cpu_temp".to_string(), "0".to_string()); // CPU temp requires extra permissions/helper, default to 0
                                vars.insert("free_memory".to_string(), convert_size(free_mem));
                                vars.insert("total_memory".to_string(), convert_size(total_mem));
                                vars.insert("memory_percent".to_string(), mem_percent.to_string());
                                vars.insert("disk_used".to_string(), convert_size(disk_used));
                                vars.insert("disk_total".to_string(), convert_size(disk_total));
                                vars.insert("disk_percent".to_string(), disk_percent.to_string());

                                let stats_text = translate(&user, "**============SYSTEM============**\n**CPU Usage:** {cpu_usage}%\n**CPU Temp:** {cpu_temp}°C\n**Free Memory:** {free_memory} of {total_memory} ({memory_percent}%)\n**Disks usage:** {disk_used} of {disk_total} ({disk_percent}%)", Some(&vars));

                                let seed_policy = settings.seed_after_download.as_str();
                                let seed_policy_label = match seed_policy {
                                    "always" => "🌱 Always",
                                    "never" => "🚫 Never",
                                    "admin_only" => "👑 Admin Only",
                                    _ => seed_policy,
                                };

                                let embed = serenity::builder::CreateEmbed::new()
                                    .title(translate(&user, "System Statistics", None))
                                    .color(0x00ae86)
                                    .description(stats_text)
                                    .field("🌿 Seeding Policy", seed_policy_label, true);

                                let _ = cmd.edit_response(&ctx.http, EditInteractionResponse::new().embed(embed)).await;
                            }
                            "speedlimit" => {
                                if user.role == "reader" {
                                    let _ = cmd.create_response(&ctx.http, CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new().content(translate(&user, "You are not authorized to use this bot", None)).ephemeral(true)
                                    )).await;
                                    return;
                                }
                                let _ = cmd.defer(&ctx.http).await;
                                match self.manager.toggle_speed_limit().await {
                                    Ok(active) => {
                                        let mode_str = if active { "ON" } else { "OFF" };
                                        let _ = cmd.edit_response(&ctx.http, EditInteractionResponse::new().content(format!("Alternate speed limits toggled: **{}**", mode_str))).await;
                                    }
                                    Err(e) => {
                                        let _ = cmd.edit_response(&ctx.http, EditInteractionResponse::new().content(format!("Error: {}", e))).await;
                                    }
                                }
                            }
                            "seeding" => {
                                if user.role != "administrator" {
                                    let _ = cmd.create_response(&ctx.http, CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new().content(translate(&user, "You are not authorized to use this bot", None)).ephemeral(true)
                                    )).await;
                                    return;
                                }

                                let policy_opt = cmd.data.options.iter().find(|o| o.name == "policy").and_then(|o| o.value.as_str());
                                match policy_opt {
                                    None => {
                                        let current = settings.seed_after_download.as_str();
                                        let label = match current {
                                            "always" => "🌱 Always (seed after every download)",
                                            "never" => "🚫 Never (stop seeding immediately)",
                                            "admin_only" => "👑 Admin Only (seed only when an admin downloads)",
                                            _ => current,
                                        };
                                        let _ = cmd.create_response(&ctx.http, CreateInteractionResponse::Message(
                                            CreateInteractionResponseMessage::new().content(format!("**Current seeding policy:** {}", label)).ephemeral(true)
                                        )).await;
                                    }
                                    Some(policy) => {
                                        let prev = settings.seed_after_download.as_str();
                                        {
                                            let mut settings_write = self.settings.write().await;
                                            settings_write.seed_after_download = policy.to_string();
                                            settings_write.export_settings();
                                        }
                                        let label_prev = match prev {
                                            "always" => "🌱 Always",
                                            "never" => "🚫 Never",
                                            "admin_only" => "👑 Admin Only",
                                            _ => prev,
                                        };
                                        let label_new = match policy {
                                            "always" => "🌱 Always",
                                            "never" => "🚫 Never",
                                            "admin_only" => "👑 Admin Only",
                                            _ => policy,
                                        };
                                        let _ = cmd.create_response(&ctx.http, CreateInteractionResponse::Message(
                                            CreateInteractionResponseMessage::new().content(format!("✅ Seeding policy updated:\n**{}** → **{}**", label_prev, label_new)).ephemeral(true)
                                        )).await;
                                    }
                                }
                            }
                            "add" => {
                                if user.role == "reader" {
                                    let _ = cmd.create_response(&ctx.http, CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new().content(translate(&user, "You are not authorized to use this bot", None)).ephemeral(true)
                                    )).await;
                                    return;
                                }

                                let _ = cmd.defer(&ctx.http).await;

                                let link_opt = cmd.data.options.iter()
                                    .find(|o| o.name == "link")
                                    .and_then(|o| o.value.as_str())
                                    .unwrap_or("");

                                let before = self.manager.get_torrents(None, None).await.unwrap_or_default();

                                let result = if link_opt.to_lowercase().starts_with("magnet:?xt=urn:") {
                                    match self.manager.add_magnet(link_opt, None).await {
                                        Ok(true) => {
                                            let hash_opt = extract_hash_from_magnet(link_opt);
                                            let mut details_msg = "✅ **Magnet link added successfully!**".to_string();
                                            if let Some(ref h) = hash_opt {
                                                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                                                if let Ok(Some(t)) = self.manager.get_torrent(h, None).await {
                                                    let peers_str = match (t.num_seeds, t.num_peers) {
                                                        (Some(seeds), Some(peers)) => format!("Seeds: `{}` | Peers: `{}`", seeds, peers),
                                                        (None, Some(peers)) => format!("Peers: `{}`", peers),
                                                        (Some(seeds), None) => format!("Seeds: `{}`", seeds),
                                                        (None, None) => "Unknown".to_string(),
                                                    };
                                                    details_msg = format!("✅ **Magnet link added successfully!**\n\n**Name:** {}\n**Size:** {}\n**Status:** `{}`\n**Peers:** {}\n**Hash:** `{}`", t.name, convert_size(t.size), t.state, peers_str, t.hash);
                                                }
                                            }
                                            Ok((details_msg, hash_opt))
                                        }
                                        Ok(false) => Err("❌ Failed to add magnet link.".to_string()),
                                        Err(e) => {
                                            if e.contains("409") {
                                                Err("⚠️ This torrent/magnet link is already in the download list.".to_string())
                                            } else {
                                                Err(format!("❌ Error: {}", e))
                                            }
                                        }
                                    }
                                } else if link_opt.to_lowercase().starts_with("http://") || link_opt.to_lowercase().starts_with("https://") {
                                    let filename = link_opt.split('/').last().unwrap_or("downloaded.torrent");
                                    let clean_filename = if filename.ends_with(".torrent") { filename } else { "downloaded.torrent" };
 
                                    match reqwest::get(link_opt).await {
                                        Ok(resp) => match resp.bytes().await {
                                            Ok(bytes) => {
                                                match self.manager.add_torrent(bytes.to_vec(), clean_filename, None).await {
                                                    Ok(true) => {
                                                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                                                        let after = self.manager.get_torrents(None, None).await.unwrap_or_default();
                                                        let new_torrent = after.iter().find(|t_after| !before.iter().any(|t_before| t_before.hash == t_after.hash));
                                                        let (details_msg, hash_opt) = if let Some(t) = new_torrent {
                                                            let peers_str = match (t.num_seeds, t.num_peers) {
                                                                (Some(seeds), Some(peers)) => format!("Seeds: `{}` | Peers: `{}`", seeds, peers),
                                                                (None, Some(peers)) => format!("Peers: `{}`", peers),
                                                                (Some(seeds), None) => format!("Seeds: `{}`", seeds),
                                                                (None, None) => "Unknown".to_string(),
                                                            };
                                                            (format!("✅ **Torrent file added successfully!**\n\n**Name:** {}\n**Size:** {}\n**Status:** `{}`\n**Peers:** {}\n**Hash:** `{}`", t.name, convert_size(t.size), t.state, peers_str, t.hash), Some(t.hash.clone()))
                                                        } else {
                                                            ("✅ Torrent file added successfully!".to_string(), None)
                                                        };
                                                        Ok((details_msg, hash_opt))
                                                    }
                                                    Ok(false) => Err("❌ Failed to add torrent file.".to_string()),
                                                    Err(e) => {
                                                        if e.contains("409") {
                                                            Err("⚠️ This torrent/magnet link is already in the download list.".to_string())
                                                        } else {
                                                            Err(format!("❌ Error: {}", e))
                                                        }
                                                    }
                                                }
                                            }
                                            Err(e) => Err(format!("Failed to read URL content: {}", e))
                                        },
                                        Err(e) => Err(format!("Failed to download from URL: {}", e))
                                    }
                                } else {
                                    Err("❌ Invalid link format. Must be a magnet link or http/https torrent URL.".to_string())
                                };

                                match result {
                                    Ok((msg, hash_opt)) => {
                                        if let Ok(resp_msg) = cmd.edit_response(&ctx.http, EditInteractionResponse::new().content(&msg)).await {
                                            if let Some(h) = hash_opt {
                                                let _ = crate::db::associate_message_with_torrent(&resp_msg.id.to_string(), &h);
                                                let manager = self.manager.clone();
                                                let http = ctx.http.clone();
                                                let cmd_clone = cmd.clone();
                                                let msg_content = msg.clone();
                                                tokio::spawn(async move {
                                                    for _ in 0..15 {
                                                        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                                                        if let Ok(Some(t)) = manager.get_torrent(&h, None).await {
                                                            if t.size > 0 && t.name != h && !t.name.is_empty() {
                                                                let peers_str = match (t.num_seeds, t.num_peers) {
                                                                    (Some(seeds), Some(peers)) => format!("Seeds: `{}` | Peers: `{}`", seeds, peers),
                                                                    (None, Some(peers)) => format!("Peers: `{}`", peers),
                                                                    (Some(seeds), None) => format!("Seeds: `{}`", seeds),
                                                                    (None, None) => "Unknown".to_string(),
                                                                };
                                                                let prefix = if msg_content.contains("Magnet link") {
                                                                    "✅ **Magnet link added successfully!**"
                                                                } else {
                                                                    "✅ **Torrent file added successfully!**"
                                                                };
                                                                let updated_msg = format!("{}\n\n**Name:** {}\n**Size:** {}\n**Status:** `{}`\n**Peers:** {}\n**Hash:** `{}`", prefix, t.name, convert_size(t.size), t.state, peers_str, t.hash);
                                                                let _ = cmd_clone.edit_response(&http, EditInteractionResponse::new().content(updated_msg)).await;
                                                                break;
                                                            }
                                                        }
                                                    }
                                                });
                                            }
                                        }
                                    }
                                    Err(msg) => {
                                        let _ = cmd.edit_response(&ctx.http, EditInteractionResponse::new().content(msg)).await;
                                    }
                                }
                            }
                            "info" => {
                                let _ = cmd.defer(&ctx.http).await;
                                let identifier = cmd.data.options.iter()
                                    .find(|o| o.name == "identifier")
                                    .and_then(|o| o.value.as_str())
                                    .unwrap_or("");

                                let mut hash = identifier.to_string();
                                if let Ok(Some(h)) = crate::db::get_torrent_hash_for_message(identifier) {
                                    hash = h;
                                }

                                let _ = self.show_torrent_details(&ctx, &interaction, &user, &hash, &settings).await;
                            }
                            "link" => {
                                let _ = cmd.defer_ephemeral(&ctx.http).await;
                                let identifier = cmd.data.options.iter()
                                    .find(|o| o.name == "identifier")
                                    .and_then(|o| o.value.as_str())
                                    .unwrap_or("");

                                let mut hash = identifier.to_string();
                                if let Ok(Some(h)) = crate::db::get_torrent_hash_for_message(identifier) {
                                    hash = h;
                                }

                                match self.manager.get_torrent(&hash, None).await {
                                    Ok(Some(torrent)) => {
                                        if torrent.progress >= 1.0 {
                                            match crate::s3::get_download_link(&settings, &torrent).await {
                                                Ok(Some(url)) => {
                                                    let row = CreateActionRow::Buttons(vec![
                                                        CreateButton::new_link(url).label("Download")
                                                    ]);
                                                    let content = if settings.local_server.enabled {
                                                        format!("🔗 Here is your download link for **{}**:", torrent.name)
                                                    } else {
                                                        format!("🔗 Here is your temporary download link for **{}**:\n*(Expires in 1 hour)*", torrent.name)
                                                    };
                                                    let _ = cmd.edit_response(&ctx.http, EditInteractionResponse::new()
                                                        .content(content)
                                                        .components(vec![row])
                                                    ).await;
                                                }
                                                Ok(None) => {
                                                    let _ = cmd.edit_response(&ctx.http, EditInteractionResponse::new().content("❌ Download links are either disabled or not configured.")).await;
                                                }
                                                Err(e) => {
                                                    let _ = cmd.edit_response(&ctx.http, EditInteractionResponse::new().content(format!("Error: {}", e))).await;
                                                }
                                            }
                                        } else {
                                            let _ = cmd.edit_response(&ctx.http, EditInteractionResponse::new().content("❌ Torrent is not fully completed yet.")).await;
                                        }
                                    }
                                    _ => {
                                        let _ = cmd.edit_response(&ctx.http, EditInteractionResponse::new().content("❌ Torrent not found.")).await;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    serenity::model::application::CommandType::Message => {
                        let target_msg_id = cmd.data.target_id.unwrap().to_string();
                        match name {
                            "Torrent Info" => {
                                let _ = cmd.defer(&ctx.http).await;
                                match crate::db::get_torrent_hash_for_message(&target_msg_id) {
                                    Ok(Some(hash)) => {
                                        let _ = self.show_torrent_details(&ctx, &interaction, &user, &hash, &settings).await;
                                    }
                                    Ok(None) => {
                                        let _ = cmd.edit_response(&ctx.http, EditInteractionResponse::new().content("❌ This message is not associated with any torrent.")).await;
                                    }
                                    Err(e) => {
                                        let _ = cmd.edit_response(&ctx.http, EditInteractionResponse::new().content(format!("❌ Database error: {}", e))).await;
                                    }
                                }
                            }
                            "Get Download Link" => {
                                let _ = cmd.defer_ephemeral(&ctx.http).await;
                                match crate::db::get_torrent_hash_for_message(&target_msg_id) {
                                    Ok(Some(hash)) => {
                                        match self.manager.get_torrent(&hash, None).await {
                                            Ok(Some(torrent)) => {
                                                if torrent.progress >= 1.0 {
                                                    match crate::s3::get_download_link(&settings, &torrent).await {
                                                        Ok(Some(url)) => {
                                                            let row = CreateActionRow::Buttons(vec![
                                                                CreateButton::new_link(url).label("Download")
                                                            ]);
                                                            let content = if settings.local_server.enabled {
                                                                format!("🔗 Here is your download link for **{}**:", torrent.name)
                                                            } else {
                                                                format!("🔗 Here is your temporary download link for **{}**:\n*(Expires in 1 hour)*", torrent.name)
                                                            };
                                                            let _ = cmd.edit_response(&ctx.http, EditInteractionResponse::new()
                                                                .content(content)
                                                                .components(vec![row])
                                                            ).await;
                                                        }
                                                        Ok(None) => {
                                                            let _ = cmd.edit_response(&ctx.http, EditInteractionResponse::new().content("❌ Download links are either disabled or not configured.")).await;
                                                        }
                                                        Err(e) => {
                                                            let _ = cmd.edit_response(&ctx.http, EditInteractionResponse::new().content(format!("Error: {}", e))).await;
                                                        }
                                                    }
                                                } else {
                                                    let _ = cmd.edit_response(&ctx.http, EditInteractionResponse::new().content("❌ Torrent is not fully completed yet.")).await;
                                                }
                                            }
                                            _ => {
                                                let _ = cmd.edit_response(&ctx.http, EditInteractionResponse::new().content("❌ Torrent not found in client.")).await;
                                            }
                                        }
                                    }
                                    Ok(None) => {
                                        let _ = cmd.edit_response(&ctx.http, EditInteractionResponse::new().content("❌ This message is not associated with any torrent.")).await;
                                    }
                                    Err(e) => {
                                        let _ = cmd.edit_response(&ctx.http, EditInteractionResponse::new().content(format!("❌ Database error: {}", e))).await;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            Interaction::Component(comp) => {
                let custom_id = comp.data.custom_id.as_str();
                if custom_id == "select_torrent" {
                    if let serenity::model::application::ComponentInteractionDataKind::StringSelect { values } = &comp.data.kind {
                        if let Some(hash) = values.first() {
                            let _ = self.show_torrent_details(&ctx, &interaction, &user, hash, &settings).await;
                        }
                    }
                } else if custom_id == "dc_back_list" {
                    if let Err(e) = self.show_torrent_list(&ctx, &interaction, &user, None).await {
                        let _ = comp.create_response(&ctx.http, CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new().content(format!("❌ Error: {}", e)).ephemeral(true)
                        )).await;
                    }
                } else if custom_id == "dc_request_access" {
                    let _ = comp.create_response(&ctx.http, CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new().content("⏳ Access request sent to administrators. Please wait...").components(vec![])
                    )).await;

                    for u in &settings.users {
                        if u.role == "administrator" && u.discord_id.is_some() && u.discord_id.as_deref() != Some("9876543210123") {
                            if let Some(ref admin_dc_id) = u.discord_id {
                                if let Ok(admin_user_id) = admin_dc_id.parse::<u64>() {
                                    if let Ok(admin_user) = UserId::new(admin_user_id).to_user(&ctx.http).await {
                                        let approve_admin = CreateButton::new(format!("dc_auth:approve:administrator:{}:{}", comp.user.id, comp.user.name))
                                            .label("Approve Admin").style(ButtonStyle::Success);
                                        let approve_manager = CreateButton::new(format!("dc_auth:approve:manager:{}:{}", comp.user.id, comp.user.name))
                                            .label("Approve Manager").style(ButtonStyle::Primary);
                                        let approve_reader = CreateButton::new(format!("dc_auth:approve:reader:{}:{}", comp.user.id, comp.user.name))
                                            .label("Approve Reader").style(ButtonStyle::Secondary);
                                        let deny_btn = CreateButton::new(format!("dc_auth:deny:{}:{}", comp.user.id, comp.user.name))
                                            .label("Deny").style(ButtonStyle::Danger);
                                        
                                        let row = CreateActionRow::Buttons(vec![approve_admin, approve_manager, approve_reader, deny_btn]);
                                        let _ = admin_user.dm(&ctx.http, serenity::builder::CreateMessage::new()
                                            .content(format!("🔔 User **{}** (ID: `{}`) is requesting access to the bot.", comp.user.name, comp.user.id))
                                            .components(vec![row])
                                        ).await;
                                    }
                                }
                            }
                        }
                    }
                } else if custom_id.starts_with("dc_auth:") {
                    let parts: Vec<&str> = custom_id.split(':').collect();
                    let action = parts[1];
                    if action == "approve" {
                        let role = parts[2];
                        let target_user_id_str = parts[3];
                        let username = parts.get(4).cloned().unwrap_or("User");
                        
                        let mut target_user = UserSettings {
                            user_id: 0,
                            discord_id: Some(target_user_id_str.to_string()),
                            role: role.to_string(),
                            locale: Some("en".to_string()),
                            notify: true,
                            notification_filter: vec![],
                        };

                        {
                            let mut settings_write = self.settings.write().await;
                            if let Some(existing) = settings_write.users.iter_mut().find(|u| u.discord_id.as_deref() == Some(target_user_id_str)) {
                                existing.role = role.to_string();
                                target_user = existing.clone();
                            } else {
                                settings_write.users.push(target_user.clone());
                            }
                            settings_write.export_settings();
                        }

                        let _ = crate::db::save_user_to_db(&target_user);

                        let _ = comp.create_response(&ctx.http, CreateInteractionResponse::UpdateMessage(
                            CreateInteractionResponseMessage::new().content(format!("✅ Authorized **{}** (ID: `{}`) as **{}**.", username, target_user_id_str, role)).components(vec![])
                        )).await;

                        if let Ok(uid) = target_user_id_str.parse::<u64>() {
                            if let Ok(tu) = UserId::new(uid).to_user(&ctx.http).await {
                                let _ = tu.dm(&ctx.http, serenity::builder::CreateMessage::new()
                                    .content(format!("🎉 Your access request has been approved! You now have **{}** role.", role))
                                ).await;
                            }
                        }
                    } else if action == "deny" {
                        let target_user_id_str = parts[2];
                        let username = parts.get(3).cloned().unwrap_or("User");

                        let _ = comp.create_response(&ctx.http, CreateInteractionResponse::UpdateMessage(
                            CreateInteractionResponseMessage::new().content(format!("❌ Access request for **{}** (ID: `{}`) was denied.", username, target_user_id_str)).components(vec![])
                        )).await;

                        if let Ok(uid) = target_user_id_str.parse::<u64>() {
                            if let Ok(tu) = UserId::new(uid).to_user(&ctx.http).await {
                                let _ = tu.dm(&ctx.http, serenity::builder::CreateMessage::new()
                                    .content("❌ Your access request was denied by an administrator.")
                                ).await;
                            }
                        }
                    }
                } else if custom_id.starts_with("dc_status:") {
                    let status = custom_id.split(':').nth(1);
                    if let Err(e) = self.show_torrent_list(&ctx, &interaction, &user, status).await {
                        let _ = comp.create_response(&ctx.http, CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new().content(format!("❌ Error: {}", e)).ephemeral(true)
                        )).await;
                    }
                } else if custom_id.starts_with("dc_get_link:") {
                    let hash = custom_id.split(':').nth(1).unwrap_or("");
                    let _ = comp.defer_ephemeral(&ctx.http).await;

                    match self.manager.get_torrent(hash, None).await {
                        Ok(Some(torrent)) => {
                            match crate::s3::get_download_link(&settings, &torrent).await {
                                Ok(Some(url)) => {
                                    let row = CreateActionRow::Buttons(vec![
                                        CreateButton::new_link(url).label("Download")
                                    ]);
                                    let content = if settings.local_server.enabled {
                                        format!("🔗 Here is your download link for **{}**:", torrent.name)
                                    } else {
                                        format!("🔗 Here is your temporary download link for **{}**:\n*(Expires in 1 hour)*", torrent.name)
                                    };
                                    let _ = comp.edit_response(&ctx.http, EditInteractionResponse::new()
                                        .content(content)
                                        .components(vec![row])
                                    ).await;
                                }
                                Ok(None) => {
                                    let _ = comp.edit_response(&ctx.http, EditInteractionResponse::new().content("❌ Download links are either disabled or not configured.")).await;
                                }
                                Err(e) => {
                                    let _ = comp.edit_response(&ctx.http, EditInteractionResponse::new().content(format!("Error: {}", e))).await;
                                }
                            }
                        }
                        _ => {
                            let _ = comp.edit_response(&ctx.http, EditInteractionResponse::new().content("Torrent not found.")).await;
                        }
                    }
                } else if custom_id.starts_with("dc_pause:") {
                    if user.role == "reader" {
                        let _ = comp.create_response(&ctx.http, CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new().content("Unauthorized.").ephemeral(true)
                        )).await;
                        return;
                    }
                    let hash = custom_id.split(':').nth(1).unwrap_or("");
                    let _ = self.manager.pause(hash).await;
                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                    let _ = self.show_torrent_details(&ctx, &interaction, &user, hash, &settings).await;
                } else if custom_id.starts_with("dc_resume:") {
                    if user.role == "reader" {
                        let _ = comp.create_response(&ctx.http, CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new().content("Unauthorized.").ephemeral(true)
                        )).await;
                        return;
                    }
                    let hash = custom_id.split(':').nth(1).unwrap_or("");
                    let _ = self.manager.resume(hash).await;
                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                    let _ = self.show_torrent_details(&ctx, &interaction, &user, hash, &settings).await;
                } else if custom_id.starts_with("dc_delete:") {
                    if user.role != "administrator" {
                        let _ = comp.create_response(&ctx.http, CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new().content("Unauthorized. Only administrators can delete torrents.").ephemeral(true)
                        )).await;
                        return;
                    }
                    let hash = custom_id.split(':').nth(1).unwrap_or("");
                    let confirm_no_data = CreateButton::new(format!("dc_cldel:{}:false", hash))
                        .label("Delete (Keep Files)").style(ButtonStyle::Danger);
                    let confirm_data = CreateButton::new(format!("dc_cldel:{}:true", hash))
                        .label("Delete EVERYTHING").style(ButtonStyle::Danger);
                    let cancel_btn = CreateButton::new(format!("dc_detail:{}", hash))
                        .label("Cancel").style(ButtonStyle::Secondary);
                    
                    let row = CreateActionRow::Buttons(vec![confirm_no_data, confirm_data, cancel_btn]);
                    let _ = comp.create_response(&ctx.http, CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new().content("⚠️ **Are you sure you want to delete this torrent?**").embeds(vec![]).components(vec![row])
                    )).await;
                } else if custom_id.starts_with("dc_cldel:") {
                    if user.role != "administrator" {
                        let _ = comp.create_response(&ctx.http, CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new().content("Unauthorized.").ephemeral(true)
                        )).await;
                        return;
                    }
                    let parts: Vec<&str> = custom_id.split(':').collect();
                    let hash = parts[1];
                    let delete_files = parts[2] == "true";

                    let res = if delete_files {
                        self.manager.delete_one_data(hash).await
                    } else {
                        self.manager.delete_one_no_data(hash).await
                    };

                    match res {
                        Ok(_) => {
                            let msg = format!("🗑️ Torrent deleted successfully{}.", if delete_files { " (including files)" } else { "" });
                            let _ = comp.create_response(&ctx.http, CreateInteractionResponse::UpdateMessage(
                                CreateInteractionResponseMessage::new().content(msg).components(vec![])
                            )).await;
                        }
                        Err(e) => {
                            let _ = comp.create_response(&ctx.http, CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new().content(format!("Failed to delete: {}", e)).ephemeral(true)
                            )).await;
                        }
                    }
                } else if custom_id.starts_with("dc_detail:") {
                    let hash = custom_id.split(':').nth(1).unwrap_or("");
                    let _ = self.show_torrent_details(&ctx, &interaction, &user, hash, &settings).await;
                }
            }
            _ => {}
        }
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }

        let is_dm = msg.guild_id.is_none();
        let bot_id = match ctx.http.get_current_user().await {
            Ok(u) => u.id,
            Err(_) => return,
        };
        let is_mentioned = msg.mentions_user_id(bot_id);

        if !is_dm && !is_mentioned {
            return;
        }

        let settings = self.settings.read().await.clone();
        let mut user = get_resolved_user(&settings, &msg.author.id.to_string());

        // Check if there is any administrator
        let has_admin = settings.users.iter().any(|u| u.role == "administrator" && u.discord_id.as_deref().unwrap_or("") != "9876543210123");
        if !has_admin {
            user.role = "administrator".to_string();
            user.discord_id = Some(msg.author.id.to_string());
            {
                let mut settings_write = self.settings.write().await;
                settings_write.users.push(user.clone());
                settings_write.export_settings();
                let _ = crate::db::save_user_to_db(&user);
            }
            let _ = msg.reply(&ctx.http, "👑 You have been automatically authorized as the first **administrator**!").await;
        } else if settings.users.iter().all(|u| u.discord_id.as_deref() != Some(&msg.author.id.to_string())) {
            let row = CreateActionRow::Buttons(vec![
                CreateButton::new("dc_request_access").label("Request Access").style(ButtonStyle::Primary)
            ]);
            let _ = msg.channel_id.send_message(&ctx.http, serenity::builder::CreateMessage::new()
                .content("❌ You are not authorized to use this bot.")
                .components(vec![row])
                .reference_message(&msg)
            ).await;
            return;
        }

        if user.role == "reader" {
            let _ = msg.reply(&ctx.http, translate(&user, "You are not authorized to use this bot", None)).await;
            return;
        }

        // 0. Check if this is a reply referencing a message
        if let Some(ref ref_msg) = msg.referenced_message {
            let text = msg.content.trim().to_lowercase();
            if text == "info" || text == "/info" || text == "link" || text == "/link" {
                let _ = msg.channel_id.broadcast_typing(&ctx.http).await;
                match crate::db::get_torrent_hash_for_message(&ref_msg.id.to_string()) {
                    Ok(Some(hash)) => {
                        if text.contains("info") {
                            match self.manager.get_torrent(&hash, None).await {
                                Ok(Some(t)) => {
                                    let progress_percent = (t.progress * 100.0).round() as i32;
                                    let embed = serenity::builder::CreateEmbed::new()
                                        .title(&t.name)
                                        .color(0x00ae86)
                                        .field("Progress", format!("{} {}%", format_progress(t.progress, 20), progress_percent), false)
                                        .field("State", format!("`{}`", t.state), true)
                                        .field("Size", format!("`{}`", convert_size(t.size)), true)
                                        .field("Download Speed", format!("`{}/s`", convert_size(t.dlspeed)), true)
                                        .field("ETA", format!("`{}`", convert_eta(t.eta)), true)
                                        .field("Hash", format!("`{}`", t.hash), false);
                                     let _ = msg.channel_id.send_message(&ctx.http, serenity::builder::CreateMessage::new().embed(embed).reference_message(&msg)).await;
                                }
                                _ => {
                                    let _ = msg.reply(&ctx.http, "❌ Torrent not found in client.").await;
                                }
                            }
                        } else {
                            match self.manager.get_torrent(&hash, None).await {
                                Ok(Some(t)) => {
                                     if t.progress >= 1.0 {
                                         match crate::s3::get_download_link(&settings, &t).await {
                                             Ok(Some(url)) => {
                                                 let row = CreateActionRow::Buttons(vec![
                                                     CreateButton::new_link(url).label("Download")
                                                 ]);
                                                 let content = if settings.local_server.enabled {
                                                     format!("🔗 Here is your download link for **{}**:", t.name)
                                                 } else {
                                                     format!("🔗 Here is your temporary download link for **{}**:\n*(Expires in 1 hour)*", t.name)
                                                 };
                                                 let _ = msg.channel_id.send_message(&ctx.http, serenity::builder::CreateMessage::new()
                                                     .content(content)
                                                     .components(vec![row])
                                                     .reference_message(&msg)
                                                 ).await;
                                             }
                                             Ok(None) => {
                                                 let _ = msg.reply(&ctx.http, "❌ Download links are either disabled or not configured.").await;
                                             }
                                             Err(e) => {
                                                 let _ = msg.reply(&ctx.http, format!("❌ Error: {}", e)).await;
                                             }
                                         }
                                     } else {
                                         let _ = msg.reply(&ctx.http, "❌ Torrent is not fully completed yet.").await;
                                     }
                                }
                                _ => {
                                    let _ = msg.reply(&ctx.http, "❌ Torrent not found in client.").await;
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        let _ = msg.reply(&ctx.http, "❌ This message is not associated with any torrent.").await;
                    }
                    Err(e) => {
                        let _ = msg.reply(&ctx.http, format!("❌ Database error: {}", e)).await;
                    }
                }
                return;
            }
        }

        // 1. Check for magnet link
        if let Some(magnet_link) = extract_clean_magnet(&msg.content) {
            let retry_row = CreateActionRow::Buttons(vec![
                CreateButton::new("dc_retry").label("Retry").style(ButtonStyle::Primary)
            ]);

            let _ = msg.channel_id.broadcast_typing(&ctx.http).await;

            match self.manager.add_magnet(&magnet_link, None).await {
                Ok(true) => {
                    let hash_opt = extract_hash_from_magnet(&magnet_link);
                    let mut details_msg = "✅ **Magnet link added successfully!**".to_string();
                    if let Some(ref h) = hash_opt {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        if let Ok(Some(t)) = self.manager.get_torrent(h, None).await {
                            let peers_str = match (t.num_seeds, t.num_peers) {
                                (Some(seeds), Some(peers)) => format!("Seeds: `{}` | Peers: `{}`", seeds, peers),
                                (None, Some(peers)) => format!("Peers: `{}`", peers),
                                (Some(seeds), None) => format!("Seeds: `{}`", seeds),
                                (None, None) => "Unknown".to_string(),
                            };
                            details_msg = format!("✅ **Magnet link added successfully!**\n\n**Name:** {}\n**Size:** {}\n**Status:** `{}`\n**Peers:** {}\n**Hash:** `{}`", t.name, convert_size(t.size), t.state, peers_str, t.hash);
                        }
                    }
                    if let Ok(mut sent_msg) = msg.reply(&ctx.http, details_msg).await {
                        if let Some(h) = hash_opt {
                            let _ = crate::db::associate_message_with_torrent(&sent_msg.id.to_string(), &h);
                            let manager = self.manager.clone();
                            let http = ctx.http.clone();
                            tokio::spawn(async move {
                                for _ in 0..15 {
                                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                                    if let Ok(Some(t)) = manager.get_torrent(&h, None).await {
                                        if t.size > 0 && t.name != h && !t.name.is_empty() {
                                            let peers_str = match (t.num_seeds, t.num_peers) {
                                                (Some(seeds), Some(peers)) => format!("Seeds: `{}` | Peers: `{}`", seeds, peers),
                                                (None, Some(peers)) => format!("Peers: `{}`", peers),
                                                (Some(seeds), None) => format!("Seeds: `{}`", seeds),
                                                (None, None) => "Unknown".to_string(),
                                            };
                                            let updated_msg = format!("✅ **Magnet link added successfully!**\n\n**Name:** {}\n**Size:** {}\n**Status:** `{}`\n**Peers:** {}\n**Hash:** `{}`", t.name, convert_size(t.size), t.state, peers_str, t.hash);
                                            let _ = sent_msg.edit(&http, serenity::builder::EditMessage::new().content(updated_msg)).await;
                                            break;
                                        }
                                    }
                                }
                            });
                        }
                    }
                }
                Ok(false) => {
                    let _ = msg.channel_id.send_message(&ctx.http, serenity::builder::CreateMessage::new()
                        .content("❌ Failed to add magnet link.")
                        .components(vec![retry_row])
                        .reference_message(&msg)
                    ).await;
                }
                Err(e) => {
                    let content = if e.contains("409") {
                        "⚠️ This torrent/magnet link is already in the download list.".to_string()
                    } else {
                        format!("❌ Error: {}", e)
                    };
                    let _ = msg.channel_id.send_message(&ctx.http, serenity::builder::CreateMessage::new()
                        .content(content)
                        .components(vec![retry_row])
                        .reference_message(&msg)
                    ).await;
                }
            }
            return;
        }

        // 2. Check for torrent attachments
        for att in &msg.attachments {
            if att.filename.ends_with(".torrent") {
                let _ = msg.channel_id.broadcast_typing(&ctx.http).await;
                
                let retry_row = CreateActionRow::Buttons(vec![
                    CreateButton::new("dc_retry").label("Retry").style(ButtonStyle::Primary)
                ]);

                let before = self.manager.get_torrents(None, None).await.unwrap_or_default();

                match reqwest::get(&att.url).await {
                    Ok(resp) => match resp.bytes().await {
                        Ok(bytes) => {
                            match self.manager.add_torrent(bytes.to_vec(), &att.filename, None).await {
                                Ok(true) => {
                                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                                    let after = self.manager.get_torrents(None, None).await.unwrap_or_default();
                                    let new_torrent = after.iter().find(|t_after| !before.iter().any(|t_before| t_before.hash == t_after.hash));
                                    let (details_msg, hash_opt) = if let Some(t) = new_torrent {
                                        let peers_str = match (t.num_seeds, t.num_peers) {
                                            (Some(seeds), Some(peers)) => format!("Seeds: `{}` | Peers: `{}`", seeds, peers),
                                            (None, Some(peers)) => format!("Peers: `{}`", peers),
                                            (Some(seeds), None) => format!("Seeds: `{}`", seeds),
                                            (None, None) => "Unknown".to_string(),
                                        };
                                        (format!("✅ **Torrent file added successfully!**\n\n**Name:** {}\n**Size:** {}\n**Status:** `{}`\n**Peers:** {}\n**Hash:** `{}`", t.name, convert_size(t.size), t.state, peers_str, t.hash), Some(t.hash.clone()))
                                    } else {
                                        ("✅ Torrent file added successfully!".to_string(), None)
                                    };
                                    if let Ok(sent_msg) = msg.reply(&ctx.http, details_msg).await {
                                        if let Some(h) = hash_opt {
                                            let _ = crate::db::associate_message_with_torrent(&sent_msg.id.to_string(), &h);
                                        }
                                    }
                                }
                                Ok(false) => {
                                    let _ = msg.channel_id.send_message(&ctx.http, serenity::builder::CreateMessage::new()
                                        .content("❌ Failed to add torrent file.")
                                        .components(vec![retry_row])
                                        .reference_message(&msg)
                                    ).await;
                                }
                                Err(e) => {
                                    let content = if e.contains("409") {
                                        "⚠️ This torrent/magnet link is already in the download list.".to_string()
                                    } else {
                                        format!("❌ Error: {}", e)
                                    };
                                    let _ = msg.channel_id.send_message(&ctx.http, serenity::builder::CreateMessage::new()
                                        .content(content)
                                        .components(vec![retry_row])
                                        .reference_message(&msg)
                                    ).await;
                                }
                            }
                        }
                        Err(e) => {
                            let _ = msg.reply(&ctx.http, format!("Failed to read attachment: {}", e)).await;
                        }
                    },
                    Err(e) => {
                        let _ = msg.reply(&ctx.http, format!("Failed to download attachment: {}", e)).await;
                    }
                }
                return;
            }
        }
    }
}

pub async fn start_discord_bot(settings: Arc<RwLock<Settings>>, manager: Arc<dyn TorrentClient>) -> Result<Client, String> {
    let token = {
        let s = settings.read().await;
        s.discord.token.clone().ok_or_else(|| "Discord token not configured".to_string())?
    };

    let intents = GatewayIntents::GUILDS 
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES 
        | GatewayIntents::MESSAGE_CONTENT;

    let handler = Handler { settings, manager };

    let client = Client::builder(&token, intents)
        .event_handler(handler)
        .await
        .map_err(|e| e.to_string())?;

    Ok(client)
}
