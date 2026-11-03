use super::{
    buf_confirm, commit_surface, letter_vk, should_claim, BufConfirm, KeyClaim,
};

fn claim(vk: u32) -> KeyClaim {
    KeyClaim {
        vk,
        has_buf: false,
        english: false,
        shift: false,
        cnpunct: true,
        trmode: 0,
        sentence_pending: false,
        cand_len: 0,
    }
}

#[test]
fn only_vk_a_through_vk_z_are_letters() {
    for vk in 0x41..=0x5A {
        assert_eq!(letter_vk(vk), Some(vk as u8 as char));
        assert!(should_claim(&KeyClaim { vk: vk as u32, ..claim(0) }));
        assert!(!should_claim(&KeyClaim {
            vk: vk as u32,
            english: true,
            ..claim(0)
        }));
    }
}

#[test]
fn numpad_function_and_truncated_keys_are_not_letters() {
    // 0x61 是小键盘 1，截成 u8 会变成 'a'；0x74 是 F5，会变成 't'；
    // 0x141 截成 u8 会变成 'A'。这些都不能进拼音。
    for vk in (0x60..=0x6F).chain(0x70..=0x87).chain([0x141usize]) {
        assert_eq!(letter_vk(vk), None, "VK {vk:#x}");
        for has_buf in [false, true] {
            assert!(
                !should_claim(&KeyClaim {
                    vk: vk as u32,
                    has_buf,
                    cand_len: 3,
                    ..claim(0)
                }),
                "VK {vk:#x} buffer={has_buf}"
            );
        }
    }
}

#[test]
fn tab_is_claimed_only_when_a_candidate_exists() {
    assert!(!should_claim(&KeyClaim {
        vk: 0x09,
        has_buf: true,
        cand_len: 0,
        ..claim(0)
    }));
    assert!(should_claim(&KeyClaim {
        vk: 0x09,
        has_buf: true,
        cand_len: 2,
        ..claim(0)
    }));
    assert!(!should_claim(&KeyClaim { vk: 0x09, ..claim(0) }));
    assert_eq!(buf_confirm(0x09, true, 0), Some(BufConfirm::PassToApp));
    assert_eq!(buf_confirm(0x09, false, 4), Some(BufConfirm::PassToApp));
    assert_eq!(buf_confirm(0x09, true, 2), None);
}

#[test]
fn horizontal_caret_keys_stay_claimed_without_candidates() {
    // v4.14：←/→、Delete、Home、End 在拼音里移动光标，没有候选也要拦。
    for vk in [0x25, 0x27, 0x2E, 0x24, 0x23] {
        assert!(should_claim(&KeyClaim {
            vk,
            has_buf: true,
            cand_len: 0,
            ..claim(0)
        }));
        assert!(!should_claim(&KeyClaim { vk, ..claim(0) }));
    }
    // ↑/↓ 没有候选就交给应用移动正文光标。
    for vk in [0x26, 0x28] {
        assert!(!should_claim(&KeyClaim {
            vk,
            has_buf: true,
            cand_len: 0,
            ..claim(0)
        }));
        assert_eq!(buf_confirm(vk, true, 0), Some(BufConfirm::PassToApp));
    }
}

#[test]
fn enter_and_space_without_candidates_commit_raw_letters() {
    for vk in [0x0D, 0x20] {
        assert!(should_claim(&KeyClaim {
            vk,
            has_buf: true,
            cand_len: 0,
            ..claim(0)
        }));
        assert_eq!(buf_confirm(vk, true, 0), Some(BufConfirm::CommitRaw));
        assert_eq!(buf_confirm(vk, true, 2), Some(BufConfirm::CommitCandidate));
        assert_eq!(buf_confirm(vk, false, 0), Some(BufConfirm::PassToApp));
    }
}

#[test]
fn commit_surface_uses_the_row_gloss_not_another_reading() {
    assert_eq!(
        commit_surface("重", " повторя́ть | повтори́ть ", true, 0, 1, false),
        "повторя́ть"
    );
    assert_eq!(
        commit_surface("重", " повторя́ть | повтори́ть ", true, 1, 0, false),
        "повтори́ть"
    );
    assert_eq!(
        commit_surface("重", " повторя́ть | повтори́ть ", false, 0, 1, false),
        "重 повторя́ть"
    );
    assert_eq!(commit_surface("重", "тяжёлый", false, 0, 0, false), "重");
    // 空释义回退汉字，不借另一条读音。
    assert_eq!(commit_surface("重", "  ", true, 3, 1, false), "重");
    assert_eq!(commit_surface("重", "", false, 0, 1, false), "重");
    // 表情没有词典译文：逐字模式原样保留；俄语侧用这一行的标签。
    assert_eq!(commit_surface("😊", "开心", false, 0, 1, true), "😊");
    assert_eq!(commit_surface("😊", "开心", true, 0, 0, true), "开心");
}
