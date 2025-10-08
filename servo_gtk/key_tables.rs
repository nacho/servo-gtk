/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use glib::translate::{FromGlib, IntoGlib};
use gtk::gdk;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum KeyLocation {
    Standard,
    Left,
    Right,
    Numpad,
}

pub struct KeyTables {
    keys: HashMap<u32, (&'static str, KeyLocation)>,
    numpad_table: Vec<u32>,
}

impl KeyTables {
    pub fn new() -> Self {
        let mut keys = HashMap::new();

        // Standard keys
        keys.insert(
            gdk::Key::MultipleCandidate.into_glib(),
            ("AllCandidates", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Eisu_Shift.into_glib(),
            ("Alphanumeric", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::ISO_Level3_Shift.into_glib(),
            ("AltGraph", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Down.into_glib(),
            ("ArrowDown", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Left.into_glib(),
            ("ArrowLeft", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Right.into_glib(),
            ("ArrowRight", KeyLocation::Standard),
        );
        keys.insert(gdk::Key::Up.into_glib(), ("ArrowUp", KeyLocation::Standard));
        keys.insert(
            gdk::Key::_3270_Attn.into_glib(),
            ("Attn", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::AudioLowerVolume.into_glib(),
            ("AudioVolumeDown", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::AudioMute.into_glib(),
            ("AudioVolumeMute", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::AudioRaiseVolume.into_glib(),
            ("AudioVolumeUp", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::BackSpace.into_glib(),
            ("Backspace", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::MonBrightnessDown.into_glib(),
            ("BrightnessDown", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::MonBrightnessUp.into_glib(),
            ("BrightnessUp", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Back.into_glib(),
            ("BrowserBack", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Forward.into_glib(),
            ("BrowserForward", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::HomePage.into_glib(),
            ("BrowserHome", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Refresh.into_glib(),
            ("BrowserRefresh", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Search.into_glib(),
            ("BrowserSearch", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Stop.into_glib(),
            ("BrowserStop", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Cancel.into_glib(),
            ("Cancel", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Caps_Lock.into_glib(),
            ("CapsLock", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Clear.into_glib(),
            ("Clear", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Close.into_glib(),
            ("Close", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Codeinput.into_glib(),
            ("CodeInput", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Red.into_glib(),
            ("ColorF0Red", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Green.into_glib(),
            ("ColorF1Green", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Yellow.into_glib(),
            ("ColorF2Yellow", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Blue.into_glib(),
            ("ColorF3Blue", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Multi_key.into_glib(),
            ("Compose", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Menu.into_glib(),
            ("ContextMenu", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Henkan.into_glib(),
            ("Convert", KeyLocation::Standard),
        );
        keys.insert(gdk::Key::Copy.into_glib(), ("Copy", KeyLocation::Standard));
        keys.insert(
            gdk::Key::_3270_CursorSelect.into_glib(),
            ("CrSel", KeyLocation::Standard),
        );
        keys.insert(gdk::Key::Cut.into_glib(), ("Cut", KeyLocation::Standard));
        keys.insert(
            gdk::Key::Delete.into_glib(),
            ("Delete", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::BrightnessAdjust.into_glib(),
            ("Dimmer", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Eject.into_glib(),
            ("Eject", KeyLocation::Standard),
        );
        keys.insert(gdk::Key::End.into_glib(), ("End", KeyLocation::Standard));
        keys.insert(
            gdk::Key::Return.into_glib(),
            ("Enter", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::_3270_EraseEOF.into_glib(),
            ("EraseEof", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Escape.into_glib(),
            ("Escape", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::_3270_ExSelect.into_glib(),
            ("ExSel", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Execute.into_glib(),
            ("Execute", KeyLocation::Standard),
        );

        // Function keys
        keys.insert(gdk::Key::F1.into_glib(), ("F1", KeyLocation::Standard));
        keys.insert(gdk::Key::F2.into_glib(), ("F2", KeyLocation::Standard));
        keys.insert(gdk::Key::F3.into_glib(), ("F3", KeyLocation::Standard));
        keys.insert(gdk::Key::F4.into_glib(), ("F4", KeyLocation::Standard));
        keys.insert(gdk::Key::F5.into_glib(), ("F5", KeyLocation::Standard));
        keys.insert(gdk::Key::F6.into_glib(), ("F6", KeyLocation::Standard));
        keys.insert(gdk::Key::F7.into_glib(), ("F7", KeyLocation::Standard));
        keys.insert(gdk::Key::F8.into_glib(), ("F8", KeyLocation::Standard));
        keys.insert(gdk::Key::F9.into_glib(), ("F9", KeyLocation::Standard));
        keys.insert(gdk::Key::F10.into_glib(), ("F10", KeyLocation::Standard));
        keys.insert(gdk::Key::F11.into_glib(), ("F11", KeyLocation::Standard));
        keys.insert(gdk::Key::F12.into_glib(), ("F12", KeyLocation::Standard));
        keys.insert(gdk::Key::F13.into_glib(), ("F13", KeyLocation::Standard));
        keys.insert(gdk::Key::F14.into_glib(), ("F14", KeyLocation::Standard));
        keys.insert(gdk::Key::F15.into_glib(), ("F15", KeyLocation::Standard));
        keys.insert(gdk::Key::F16.into_glib(), ("F16", KeyLocation::Standard));
        keys.insert(gdk::Key::F17.into_glib(), ("F17", KeyLocation::Standard));
        keys.insert(gdk::Key::F18.into_glib(), ("F18", KeyLocation::Standard));
        keys.insert(gdk::Key::F19.into_glib(), ("F19", KeyLocation::Standard));
        keys.insert(gdk::Key::F20.into_glib(), ("F20", KeyLocation::Standard));
        keys.insert(gdk::Key::F21.into_glib(), ("F21", KeyLocation::Standard));
        keys.insert(gdk::Key::F22.into_glib(), ("F22", KeyLocation::Standard));
        keys.insert(gdk::Key::F23.into_glib(), ("F23", KeyLocation::Standard));
        keys.insert(gdk::Key::F24.into_glib(), ("F24", KeyLocation::Standard));
        keys.insert(gdk::Key::F25.into_glib(), ("F25", KeyLocation::Standard));
        keys.insert(gdk::Key::F26.into_glib(), ("F26", KeyLocation::Standard));
        keys.insert(gdk::Key::F27.into_glib(), ("F27", KeyLocation::Standard));
        keys.insert(gdk::Key::F28.into_glib(), ("F28", KeyLocation::Standard));
        keys.insert(gdk::Key::F29.into_glib(), ("F29", KeyLocation::Standard));
        keys.insert(gdk::Key::F30.into_glib(), ("F30", KeyLocation::Standard));
        keys.insert(gdk::Key::F31.into_glib(), ("F31", KeyLocation::Standard));
        keys.insert(gdk::Key::F32.into_glib(), ("F32", KeyLocation::Standard));
        keys.insert(gdk::Key::F33.into_glib(), ("F33", KeyLocation::Standard));
        keys.insert(gdk::Key::F34.into_glib(), ("F34", KeyLocation::Standard));
        keys.insert(gdk::Key::F35.into_glib(), ("F35", KeyLocation::Standard));

        // More standard keys
        keys.insert(gdk::Key::Find.into_glib(), ("Find", KeyLocation::Standard));
        keys.insert(
            gdk::Key::ISO_First_Group.into_glib(),
            ("GroupFirst", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::ISO_Last_Group.into_glib(),
            ("GroupLast", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::ISO_Next_Group.into_glib(),
            ("GroupNext", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::ISO_Prev_Group.into_glib(),
            ("GroupPrevious", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Hankaku.into_glib(),
            ("Hankaku", KeyLocation::Standard),
        );
        keys.insert(gdk::Key::Help.into_glib(), ("Help", KeyLocation::Standard));
        keys.insert(
            gdk::Key::Hibernate.into_glib(),
            ("Hibernate", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Hiragana.into_glib(),
            ("Hiragana", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Hiragana_Katakana.into_glib(),
            ("HiraganaKatakana", KeyLocation::Standard),
        );
        keys.insert(gdk::Key::Home.into_glib(), ("Home", KeyLocation::Standard));
        keys.insert(
            gdk::Key::Insert.into_glib(),
            ("Insert", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Kana_Lock.into_glib(),
            ("KanaMode", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Kanji.into_glib(),
            ("KanjiMode", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Katakana.into_glib(),
            ("Katakana", KeyLocation::Standard),
        );

        // Launch keys
        keys.insert(
            gdk::Key::Calculator.into_glib(),
            ("LaunchCalculator", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Calendar.into_glib(),
            ("LaunchCalendar", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Mail.into_glib(),
            ("LaunchMail", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::CD.into_glib(),
            ("LaunchMediaPlayer", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Music.into_glib(),
            ("LaunchMusicPlayer", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::MyComputer.into_glib(),
            ("LaunchMyComputer", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::ScreenSaver.into_glib(),
            ("LaunchScreenSaver", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Excel.into_glib(),
            ("LaunchSpreadsheet", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::WWW.into_glib(),
            ("LaunchWebBrowser", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::WebCam.into_glib(),
            ("LaunchWebCam", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Word.into_glib(),
            ("LaunchWordProcessor", KeyLocation::Standard),
        );

        keys.insert(
            gdk::Key::LogOff.into_glib(),
            ("LogOff", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::MailForward.into_glib(),
            ("MailForward", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Reply.into_glib(),
            ("MailReply", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Send.into_glib(),
            ("MailSend", KeyLocation::Standard),
        );

        // Media keys
        keys.insert(
            gdk::Key::AudioForward.into_glib(),
            ("MediaFastForward", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::AudioPause.into_glib(),
            ("MediaPause", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::AudioPlay.into_glib(),
            ("MediaPlay", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::AudioRecord.into_glib(),
            ("MediaRecord", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::AudioRewind.into_glib(),
            ("MediaRewind", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::AudioStop.into_glib(),
            ("MediaStop", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::AudioNext.into_glib(),
            ("MediaTrackNext", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::AudioPrev.into_glib(),
            ("MediaTrackPrevious", KeyLocation::Standard),
        );

        keys.insert(gdk::Key::New.into_glib(), ("New", KeyLocation::Standard));
        keys.insert(
            gdk::Key::Muhenkan.into_glib(),
            ("NonConvert", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Num_Lock.into_glib(),
            ("NumLock", KeyLocation::Standard),
        );
        keys.insert(gdk::Key::Open.into_glib(), ("Open", KeyLocation::Standard));
        keys.insert(
            gdk::Key::Page_Down.into_glib(),
            ("PageDown", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Page_Up.into_glib(),
            ("PageUp", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Paste.into_glib(),
            ("Paste", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Pause.into_glib(),
            ("Pause", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::_3270_Play.into_glib(),
            ("Play", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::PowerOff.into_glib(),
            ("PowerOff", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::PreviousCandidate.into_glib(),
            ("PreviousCandidate", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Print.into_glib(),
            ("PrintScreen", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::AudioRandomPlay.into_glib(),
            ("RandomToggle", KeyLocation::Standard),
        );
        keys.insert(gdk::Key::Redo.into_glib(), ("Redo", KeyLocation::Standard));
        keys.insert(
            gdk::Key::Romaji.into_glib(),
            ("Romaji", KeyLocation::Standard),
        );
        keys.insert(gdk::Key::Save.into_glib(), ("Save", KeyLocation::Standard));
        keys.insert(
            gdk::Key::Scroll_Lock.into_glib(),
            ("ScrollLock", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Select.into_glib(),
            ("Select", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Shift_Lock.into_glib(),
            ("Shift", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::SingleCandidate.into_glib(),
            ("SingleCandidate", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Spell.into_glib(),
            ("SpellCheck", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Standby.into_glib(),
            ("Standby", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Subtitle.into_glib(),
            ("Subtitle", KeyLocation::Standard),
        );
        keys.insert(gdk::Key::Tab.into_glib(), ("Tab", KeyLocation::Standard));
        keys.insert(gdk::Key::Undo.into_glib(), ("Undo", KeyLocation::Standard));
        keys.insert(
            gdk::Key::Next_VMode.into_glib(),
            ("VideoModeNext", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::WakeUp.into_glib(),
            ("WakeUp", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Zenkaku.into_glib(),
            ("Zenkaku", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::Zenkaku_Hankaku.into_glib(),
            ("ZenkakuHankaku", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::ZoomIn.into_glib(),
            ("ZoomIn", KeyLocation::Standard),
        );
        keys.insert(
            gdk::Key::ZoomOut.into_glib(),
            ("ZoomOut", KeyLocation::Standard),
        );

        // Left keys
        keys.insert(gdk::Key::Alt_L.into_glib(), ("Alt", KeyLocation::Left));
        keys.insert(
            gdk::Key::Control_L.into_glib(),
            ("Control", KeyLocation::Left),
        );
        keys.insert(gdk::Key::Meta_L.into_glib(), ("Meta", KeyLocation::Left));
        keys.insert(gdk::Key::Super_L.into_glib(), ("OS", KeyLocation::Left));
        keys.insert(gdk::Key::Shift_L.into_glib(), ("Shift", KeyLocation::Left));
        keys.insert(
            gdk::Key::ISO_Left_Tab.into_glib(),
            ("Tab", KeyLocation::Left),
        );

        // Right keys
        keys.insert(gdk::Key::Alt_R.into_glib(), ("Alt", KeyLocation::Right));
        keys.insert(
            gdk::Key::Control_R.into_glib(),
            ("Control", KeyLocation::Right),
        );
        keys.insert(gdk::Key::Meta_R.into_glib(), ("Meta", KeyLocation::Right));
        keys.insert(gdk::Key::Super_R.into_glib(), ("OS", KeyLocation::Right));
        keys.insert(gdk::Key::Shift_R.into_glib(), ("Shift", KeyLocation::Right));

        // Numpad keys
        keys.insert(
            gdk::Key::KP_Down.into_glib(),
            ("ArrowDown", KeyLocation::Numpad),
        );
        keys.insert(
            gdk::Key::KP_Left.into_glib(),
            ("ArrowLeft", KeyLocation::Numpad),
        );
        keys.insert(
            gdk::Key::KP_Right.into_glib(),
            ("ArrowRight", KeyLocation::Numpad),
        );
        keys.insert(
            gdk::Key::KP_Up.into_glib(),
            ("ArrowUp", KeyLocation::Numpad),
        );
        keys.insert(
            gdk::Key::KP_Delete.into_glib(),
            ("Delete", KeyLocation::Numpad),
        );
        keys.insert(gdk::Key::KP_End.into_glib(), ("End", KeyLocation::Numpad));
        keys.insert(
            gdk::Key::KP_Enter.into_glib(),
            ("Enter", KeyLocation::Numpad),
        );
        keys.insert(gdk::Key::KP_F1.into_glib(), ("F1", KeyLocation::Numpad));
        keys.insert(gdk::Key::KP_F2.into_glib(), ("F2", KeyLocation::Numpad));
        keys.insert(gdk::Key::KP_F3.into_glib(), ("F3", KeyLocation::Numpad));
        keys.insert(gdk::Key::KP_F4.into_glib(), ("F4", KeyLocation::Numpad));
        keys.insert(gdk::Key::KP_Home.into_glib(), ("Home", KeyLocation::Numpad));
        keys.insert(
            gdk::Key::KP_Insert.into_glib(),
            ("Insert", KeyLocation::Numpad),
        );
        keys.insert(
            gdk::Key::KP_Page_Down.into_glib(),
            ("PageDown", KeyLocation::Numpad),
        );
        keys.insert(
            gdk::Key::KP_Page_Up.into_glib(),
            ("PageUp", KeyLocation::Numpad),
        );
        keys.insert(gdk::Key::KP_Tab.into_glib(), ("Tab", KeyLocation::Numpad));

        let numpad_table = vec![
            gdk::Key::KP_Enter.into_glib(),
            gdk::Key::KP_Multiply.into_glib(),
            gdk::Key::KP_Add.into_glib(),
            gdk::Key::KP_Separator.into_glib(),
            gdk::Key::KP_Subtract.into_glib(),
            gdk::Key::KP_Decimal.into_glib(),
            gdk::Key::KP_Divide.into_glib(),
            gdk::Key::KP_0.into_glib(),
            gdk::Key::KP_1.into_glib(),
            gdk::Key::KP_2.into_glib(),
            gdk::Key::KP_3.into_glib(),
            gdk::Key::KP_4.into_glib(),
            gdk::Key::KP_5.into_glib(),
            gdk::Key::KP_6.into_glib(),
            gdk::Key::KP_7.into_glib(),
            gdk::Key::KP_8.into_glib(),
            gdk::Key::KP_9.into_glib(),
        ];

        Self { keys, numpad_table }
    }

    pub fn key_from_keyval(&self, keyval: u32) -> Option<(String, bool, KeyLocation)> {
        if let Some((key_name, location)) = self.keys.get(&keyval) {
            Some((key_name.to_string(), false, location.clone()))
        } else {
            // Try to convert to unicode character
            let gdk_key = unsafe { gdk::Key::from_glib(keyval) };
            if let Some(unicode_char) = gdk_key.to_unicode()
                && unicode_char != '\0'
            {
                let location = if self.numpad_table.contains(&keyval) {
                    KeyLocation::Numpad
                } else {
                    KeyLocation::Standard
                };
                return Some((unicode_char.to_string(), true, location));
            }
            None
        }
    }
}

impl Default for KeyTables {
    fn default() -> Self {
        Self::new()
    }
}
