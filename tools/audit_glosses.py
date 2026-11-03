#!/usr/bin/env python3
"""Audit Russian glosses on the most frequent Chinese headwords.

The frequency ranking and the 200-word sample are computed from the files
in the repo. The four quality labels are human judgments stored below
(JUDGMENTS); re-running this script only checks that those labels still
name exactly the sampled words. It does not modify any data file.

Ranking
  File: ime/src/dict.tsv (the table compiled into the IME).
  One headword = one distinct simplified string.
  freq(word) = maximum integer in the freq column across its rows.
  That column is the jieba frequency baked in by tools/build_dict.py
  (including FREQ_BOOST). jieba_dict.txt itself is not in the repo.
  Sort by freq descending, then by the simplified string's code points
  ascending. Keep the first 5000.
  Words tied on the cutoff frequency stay in or out together only as
  this sort places them; the script prints the cutoff so the edge is
  visible.

Sample
  Of those 5000, the 200 whose SHA-256 of
  "{SEED}\\t{simplified}".encode("utf-8") is smallest, compared as hex.
  SEED = 20261103. This does not use random.sample, so the set does not
  depend on the Python version.

Russian gloss
  A reading counts as Russian when its ru field contains a Cyrillic
  letter. A leading "en:" English fallback does not.
  Stress is U+0301 (combining acute), the mark already used in the table.
"""

from __future__ import annotations

import hashlib
import os
import sys
from collections import Counter, defaultdict

SEED = 20261103
TOP_N = 5000
SAMPLE_N = 200

CYR = set("абвгдеёжзийклмнопрстуфхцчшщъыьэюяАБВГДЕЁЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯ")
VOWELS = set("аеёиоуыэюяАЕЁИОУЫЭЮЯ")
STRESS = "\u0301"
# First |-segment that is only a function word: a mechanical hint that a
# multi-word Russian phrase was split into fake senses. Not a quality label.
BARE_FUNCTION = {
    "в", "на", "и", "с", "к", "за", "по", "от", "до", "из", "у", "о",
    "быть", "не", "для", "это",
}

# Human labels for the seeded sample. label is one of:
#   correct, acceptable, wrong, garbage
# unsure=True means the boundary between two labels was not clear.
# Notes are the reason, in Chinese, for anything that is not simply correct.
JUDGMENTS = [
    ("为", "correct", False, ""),
    ("上", "correct", False, ""),
    ("又", "correct", False, ""),
    ("得", "wrong", False, "标成 dé，释义却是程度补语；děi 写成「得到」，没有「必须」"),
    ("被", "correct", False, ""),
    ("主要", "correct", False, ""),
    ("全国", "acceptable", False, "всенародный，缺「整个国家」"),
    ("这种", "correct", False, ""),
    ("比", "acceptable", False, "只有名词「比较」，介词「比……」不在"),
    ("即", "correct", False, ""),
    ("跟", "correct", False, ""),
    ("一般", "acceptable", True, "只有 одинаковый（一样），缺「普通」"),
    ("所以", "wrong", False, "首义 зато（反而），потому 排在第三"),
    ("曾", "wrong", False, "首义 как только；曾经应是 когда-то"),
    ("方面", "wrong", False, "英文 respect 被枢成「尊敬」"),
    ("市场", "acceptable", False, "рынок 对，магазин（商店）偏了"),
    ("之间", "correct", False, ""),
    ("一定", "correct", False, ""),
    ("具有", "garbage", False, "быть | в | вмещать | двусложным，没有「具有」"),
    ("建立", "wrong", False, "首义 назначить（任命），установить 在后"),
    ("我国", "correct", False, ""),
    ("以后", "acceptable", False, "после 对，多了 в поисках（寻找）"),
    ("均", "acceptable", False, "混进 униформа：英文 uniform 被当成制服"),
    ("种", "acceptable", False, "只有 zhǒng「种类」，没有 zhòng「种植」"),
    ("东西", "correct", False, ""),
    ("实现", "acceptable", False, "短语被拆开，首义是 в；осуществлять 还在"),
    ("形式", "wrong", False, "首义 бланк（表格），英文 form；форма 排第三"),
    ("那里", "correct", False, ""),
    ("整个", "correct", False, ""),
    ("规模", "correct", False, ""),
    ("石", "wrong", False, "首义 рок（摇滚）；残片「горная )」"),
    ("进一步", "correct", False, ""),
    ("严重", "correct", False, ""),
    ("商品", "correct", False, ""),
    ("事业", "wrong", False, "英文 undertaking 的殡葬义：ритуальные услуги"),
    ("目的", "correct", False, ""),
    ("现象", "correct", False, ""),
    ("胜利", "correct", False, ""),
    ("意义", "correct", False, ""),
    ("官员", "correct", False, ""),
    ("乱", "correct", False, ""),
    ("综合", "correct", False, ""),
    ("半", "correct", False, ""),
    ("位置", "correct", False, ""),
    ("毛", "acceptable", True, "毛发对；「1/1000」这个单位义没有把握"),
    ("忽然", "acceptable", False, "вдруг 对，мельком 不对"),
    ("喜", "correct", False, ""),
    ("南京", "correct", False, ""),
    ("内部", "acceptable", True, "首义像「内地」；внутренний 在第二"),
    ("数量", "correct", False, ""),
    ("中国共产党", "correct", False, ""),
    ("公元", "garbage", False, "拆成「日期 / 欧洲的 / 纪年 / н.」，读不出公元"),
    ("团", "correct", False, ""),
    ("文明", "correct", False, ""),
    ("句", "acceptable", False, "jù 对；gōu 被贴了同一条，并夹有英文 Классификатор"),
    ("真的", "correct", False, ""),
    ("胜", "correct", False, ""),
    ("例如", "correct", False, ""),
    ("怪", "correct", False, ""),
    ("战斗", "wrong", False, "首义 драка（打架），бой 排在第三"),
    ("降", "wrong", False, "xiáng（投降）被贴成「下降」，没有投降"),
    ("责任", "correct", False, ""),
    ("臣", "correct", False, ""),
    ("西部", "correct", False, ""),
    ("事务", "acceptable", True, "дела 在后面；首片只是形容词 административные"),
    ("不了", "acceptable", False, "buliao 大致是「不能」；bule 被译成 спасибо"),
    ("更是", "garbage", False, "в | главные | образом | особенности，对不上「更是」"),
    ("旁", "wrong", False, "другой（别的），不是「旁边」"),
    ("武", "correct", False, ""),
    ("硬", "correct", False, ""),
    ("常常", "correct", False, ""),
    ("一部分", "correct", False, ""),
    ("苦", "acceptable", False, "горький 对，后面重复梵文 дуккха"),
    ("满足", "acceptable", False, "首义 быть，довольствоваться 被拆到后面"),
    ("广", "correct", False, ""),
    ("来看", "correct", False, ""),
    ("轮", "correct", False, ""),
    ("宝", "correct", False, ""),
    ("发布会", "correct", False, ""),
    ("失去", "acceptable", False, "потерять 对，нерадеть 多余"),
    ("成本", "correct", False, ""),
    ("步", "correct", False, ""),
    ("财富", "acceptable", False, "首义 благополучие（福祉），богатство 在第三"),
    ("观念", "correct", False, ""),
    ("运用", "acceptable", False, "首义 в；пользоваться 在后面"),
    ("抓住", "acceptable", False, "поймать 对，后面夹了单独的 за"),
    ("作者", "acceptable", False, "писатель 偏小说家，作者更常是 автор"),
    ("温度", "correct", False, ""),
    ("费用", "correct", False, ""),
    ("一口", "wrong", False, "只有 охотно（一口答应），没有「一口」"),
    ("挺", "correct", False, ""),
    ("各类", "acceptable", False, "「各种类别」被拆成碎片"),
    ("帝国", "correct", False, ""),
    ("劝", "correct", False, ""),
    ("此次", "acceptable", False, "首义「在这个时候」，「这一次」在后"),
    ("发行", "correct", False, ""),
    ("医疗", "correct", False, ""),
    ("韩", "correct", False, ""),
    ("采访", "correct", False, ""),
    ("另一方面", "wrong", False, "首义 зато，с другой стороны 在后"),
    ("太太", "correct", False, ""),
    ("大概", "acceptable", False, "грубо 勉强；неровно 是「不平整」"),
    ("毕业生", "acceptable", False, "выпускник 对，后面是学校短语的碎片"),
    ("人体", "wrong", True, "只剩 человеческий（人的），тело 丢了"),
    ("仗", "correct", False, ""),
    ("上级", "acceptable", False, "首义 верхний，вышестоящий 更贴"),
    ("资产", "correct", False, ""),
    ("春秋", "correct", False, ""),
    ("日益", "acceptable", True, "像「всё более」被拆开，中间夹了 в"),
    ("居然", "garbage", False, "благополучно / вопреки / кто，没有「竟然」"),
    ("陕西", "correct", False, ""),
    ("本次", "correct", False, ""),
    ("奶奶", "acceptable", False, "首义 баба 有歧义，бабушка 在第二"),
    ("午门", "correct", False, ""),
    ("遗产", "correct", False, ""),
    ("头上", "acceptable", False, "наверху 勉强；накладные расходы 是 overhead 的费用义"),
    ("联合国", "correct", False, ""),
    ("结婚", "correct", False, ""),
    ("前进", "correct", False, ""),
    ("枚", "acceptable", False, "只写奖章，枚还用于硬币、戒指等"),
    ("贯彻", "garbage", False, "быть | в | деле | до，没有「贯彻」"),
    ("独", "correct", False, ""),
    ("临", "correct", False, ""),
    ("丝", "correct", False, ""),
    ("识", "acceptable", False, "shí 可以；zhì 被枢成唱片"),
    ("海外", "correct", False, ""),
    ("万岁", "correct", False, ""),
    ("好好", "garbage", False, "в | всё | как | когда，没有「好好地」"),
    ("学位", "acceptable", False, "首义 диплом（文凭），учёная степень 在第三"),
    ("禁止", "correct", False, ""),
    ("事项", "wrong", False, "首义 материя（物质），英文 matter"),
    ("连接", "correct", False, ""),
    ("吴三桂", "correct", False, ""),
    ("当前", "correct", False, ""),
    ("扩张", "garbage", False, "首义 желудка（胃的），短语被拆碎"),
    ("分裂", "acceptable", False, "делить 偏「分配」"),
    ("无数", "acceptable", False, "бесчисленный 对，后面有碎片"),
    ("役", "correct", False, ""),
    ("科学技术", "correct", False, ""),
    ("指数", "acceptable", False, "首义 знак，индекс 在第二"),
    ("单独", "acceptable", False, "одинокий 偏「孤独」，语法也不完整"),
    ("工作人员", "correct", False, ""),
    ("矿", "correct", False, ""),
    ("五一", "correct", False, ""),
    ("球队", "correct", False, ""),
    ("亡", "acceptable", False, "首义逃跑、丢失，缺「死亡」"),
    ("林区", "acceptable", False, "лесной | массив | район，短语被拆"),
    ("明星", "wrong", False, "首义 Юпитер（木星）；звезда 在第二"),
    ("车程", "acceptable", True, "расстояние в пути 是距离，车程常指耗时"),
    ("鏉", "garbage", True, "生僻字被塞进英文 sharp 的一堆义项；本义不确定"),
    ("服从", "correct", False, ""),
    ("陈家洛", "correct", False, ""),
    ("抛", "correct", False, ""),
    ("爷爷", "correct", False, ""),
    ("一颗", "correct", False, ""),
    ("取代", "correct", False, ""),
    ("极其", "correct", False, ""),
    ("基因", "correct", False, ""),
    ("舞台", "correct", False, ""),
    ("景", "acceptable", False, "大体对，夹了英文 Классификатор"),
    ("血液", "correct", False, ""),
    ("损害", "correct", False, ""),
    ("明日", "correct", False, ""),
    ("面向", "acceptable", False, "к... | лицом | повернуться，能还原"),
    ("情报", "acceptable", False, "информация 对，разведывательные 是碎片"),
    ("榜", "wrong", False, "bǎng 只给了「桨」，缺「榜单」"),
    ("承诺", "wrong", False, "汇票用语 акцепт / вексель，没有「答应」"),
    ("事儿", "wrong", False, "предприятие / фирма；英文 business 的公司义"),
    ("利害", "correct", False, ""),
    ("管辖", "acceptable", False, "управлять 偏「管理」"),
    ("人为", "correct", False, ""),
    ("皇家", "acceptable", False, "императорский дом 被拆开"),
    ("胡同", "correct", False, ""),
    ("一双", "acceptable", False, "写成「一双鞋」，范围偏窄"),
    ("肿瘤", "acceptable", False, "мед. 被当成一条释义"),
    ("刘备", "correct", False, ""),
    ("施行", "correct", False, ""),
    ("大国", "garbage", False, "Пауэр，不是「大国、强国」"),
    ("推翻", "acceptable", True, "опрокидывать 在后；前面是 гипотезы 和 напр."),
    ("模拟", "acceptable", False, "имитация 能用，偏「模仿」"),
    ("毫不", "garbage", False, "首义 волос（毫毛），否定式没译出来"),
    ("次日", "acceptable", False, "завтра 偏窄，次日是「第二天」"),
    ("运动员", "correct", False, ""),
    ("工商", "correct", False, ""),
    ("统帅", "garbage", True, "拆成 верховный | во | возглавлять，没有「统帅」"),
    ("佳肴", "acceptable", False, "блюдо 太泛，деликатес 在后"),
    ("清政府", "correct", False, ""),
    ("固然", "acceptable", False, "конечно 可以，混进 корреспондирующим"),
    ("海峡", "correct", False, ""),
    ("水泥", "correct", False, ""),
    ("海岸", "acceptable", False, "词内竖线把 море|моря 拆开了"),
    ("说起", "acceptable", False, "говорить / касаясь，能懂但不精确"),
    ("湪", "garbage", False, "两条都是 nogloss，释义为空"),
    ("当天", "acceptable", True, "в | день | же | самый，要自己拼回「当天」"),
    ("鄂州", "correct", False, ""),
    ("报考", "acceptable", True, "首义 в，записаться 在后面"),
    ("王公", "acceptable", False, "аристократы 对，后面夹了 и | император"),
    ("波兰", "correct", False, ""),
    ("暗中", "correct", False, ""),
    ("拐", "correct", False, ""),
]


def repo_root() -> str:
    return os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def has_cyr(text: str) -> bool:
    return any(ch in CYR for ch in text)


def is_russian(text: str) -> bool:
    body = text.strip()
    if body.lower().startswith("en:"):
        return False
    return has_cyr(body)


def load_dict(path: str) -> dict[str, list[list[str]]]:
    by: dict[str, list[list[str]]] = defaultdict(list)
    with open(path, encoding="utf-8") as fh:
        header = fh.readline()
        if not header.startswith("#simplified"):
            raise SystemExit(f"unexpected header in {path}")
        for line in fh:
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 7 or not parts[0]:
                continue
            while len(parts) < 9:
                parts.append("")
            by[parts[0]].append(parts[:9])
    return by


def freq_of(rows: list[list[str]]) -> int:
    best = 0
    for row in rows:
        try:
            best = max(best, int(row[5]))
        except ValueError:
            continue
    return best


def ranked_words(by: dict[str, list[list[str]]]) -> list[tuple[int, str, list[list[str]]]]:
    items = [(freq_of(rows), word, rows) for word, rows in by.items()]
    items.sort(key=lambda item: (-item[0], item[1]))
    return items


def sample_key(word: str) -> str:
    return hashlib.sha256(f"{SEED}\t{word}".encode("utf-8")).hexdigest()


def first_segment(gloss: str) -> str:
    return gloss.split("|", 1)[0].strip().strip(".,;:").lower()


def main() -> None:
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    root = repo_root()
    by = load_dict(os.path.join(root, "ime", "src", "dict.tsv"))
    items = ranked_words(by)
    positive = sum(1 for freq, _word, _rows in items if freq > 0)
    freq_mismatch = 0
    for _word, rows in by.items():
        seen = set()
        for row in rows:
            try:
                seen.add(int(row[5]))
            except ValueError:
                seen.add(row[5])
        if len(seen) > 1:
            freq_mismatch += 1
    uni_path = os.path.join(root, "ime", "src", "lm_uni.tsv")
    uni = set()
    with open(uni_path, encoding="utf-8") as fh:
        for line in fh:
            uni.add(line.split("\t", 1)[0])
    top = items[:TOP_N]
    cutoff = top[-1][0]
    tied = [word for freq, word, _rows in items if freq == cutoff]
    inside = [word for freq, word, _rows in top if freq == cutoff]

    nonempty = russian = stressed_words = empty_words = en_only = 0
    poly = poly_same = poly_partial = semicolon_words = 0
    readings = readings_ru = readings_empty = readings_stress = 0
    senses = senses_stress = poly_senses = poly_senses_stress = 0
    bare_function_words = 0
    nonempty_not_russian: list[str] = []
    src_rows: Counter[str] = Counter()
    src_ru: Counter[str] = Counter()
    src_stress: Counter[str] = Counter()
    empty_list: list[tuple[int, str]] = []

    for freq, word, rows in top:
        readings += len(rows)
        any_nonempty = any_ru = any_stress = any_bare = False
        any_en = False
        glosses: list[str] = []
        for row in rows:
            gloss = row[6].strip()
            src = row[8].strip() or "(empty)"
            src_rows[src] += 1
            glosses.append(gloss)
            if not gloss:
                readings_empty += 1
            else:
                any_nonempty = True
            if gloss.lower().startswith("en:"):
                any_en = True
            if is_russian(gloss):
                any_ru = True
                readings_ru += 1
                src_ru[src] += 1
                if STRESS in gloss:
                    any_stress = True
                    readings_stress += 1
                    src_stress[src] += 1
                if first_segment(gloss) in BARE_FUNCTION:
                    any_bare = True
            for part in (piece.strip() for piece in gloss.split("|")):
                if not is_russian(part):
                    continue
                senses += 1
                if STRESS in part:
                    senses_stress += 1
                bare = part.replace(STRESS, "")
                if sum(ch in VOWELS for ch in bare) >= 2:
                    poly_senses += 1
                    if STRESS in part:
                        poly_senses_stress += 1
        if any_nonempty:
            nonempty += 1
        else:
            empty_words += 1
            empty_list.append((freq, word))
        if any_ru:
            russian += 1
        if any_stress:
            stressed_words += 1
        if any_en and not any_ru:
            en_only += 1
        if any_bare:
            bare_function_words += 1
        if any_nonempty and not any_ru:
            nonempty_not_russian.append(word)
        if any(";" in gloss or "；" in gloss for gloss in glosses):
            semicolon_words += 1
        if len(rows) > 1:
            poly += 1
            if len(set(glosses)) == 1 and glosses[0]:
                poly_same += 1
            if any(glosses) and not all(glosses):
                poly_partial += 1

    sampled = sorted(top, key=lambda item: sample_key(item[1]))[:SAMPLE_N]
    rank = {word: index for index, (_freq, word, _rows) in enumerate(top, start=1)}
    sampled.sort(key=lambda item: rank[item[1]])
    sample_words = [word for _freq, word, _rows in sampled]
    fingerprint = hashlib.sha256("\n".join(sample_words).encode("utf-8")).hexdigest()

    judged = {word: (label, unsure, note) for word, label, unsure, note in JUDGMENTS}
    missing = [word for word in sample_words if word not in judged]
    extra = [word for word, _label, _unsure, _note in JUDGMENTS if word not in set(sample_words)]
    if missing or extra or len(JUDGMENTS) != len(sample_words):
        raise SystemExit(
            "JUDGMENTS do not match the sample: "
            f"missing={missing} extra={extra} n_labels={len(JUDGMENTS)}"
        )
    labels = Counter(label for _word, label, _unsure, _note in JUDGMENTS)
    unsure_n = sum(1 for _word, _label, unsure, _note in JUDGMENTS if unsure)

    # Drift against data/zh-ru.tsv. build_dict.py writes that file, not dict.tsv.
    zh: dict[tuple[str, str], str] = {}
    zh_path = os.path.join(root, "data", "zh-ru.tsv")
    with open(zh_path, encoding="utf-8") as fh:
        next(fh)
        for line in fh:
            parts = line.rstrip("\n").split("\t")
            if len(parts) >= 7:
                zh[(parts[0], parts[2])] = parts[6]
    same = differ = only_dict = 0
    for _freq, word, rows in top:
        for row in rows:
            if row[8].strip() == "nogloss":
                continue
            key = (row[0], row[2])
            if key not in zh:
                only_dict += 1
            elif zh[key].strip() == row[6].strip():
                same += 1
            else:
                differ += 1

    pct = lambda num, den: (100.0 * num / den) if den else 0.0

    print(f"headwords in dict.tsv: {len(items)}")
    print(f"headwords with freq > 0: {positive}")
    print(f"headwords whose rows disagree on freq: {freq_mismatch}")
    missing_uni = [word for _freq, word, _rows in top if word not in uni]
    print(f"top {TOP_N} also in lm_uni.tsv: {TOP_N - len(missing_uni)}")
    print(f"top {TOP_N} absent from lm_uni.tsv: {len(missing_uni)} ({'、'.join(missing_uni)})")
    print(f"rank 1: {top[0][1]} freq={top[0][0]}")
    print(f"rank {TOP_N}: {top[-1][1]} freq={cutoff}")
    print(f"next: {items[TOP_N][1]} freq={items[TOP_N][0]}")
    print(f"headwords at cutoff freq {cutoff}: {len(tied)}; inside the top {TOP_N}: {len(inside)} ({', '.join(inside)})")
    print()
    print(f"word coverage, top {TOP_N}")
    print(f"  non-empty ru field: {nonempty}/{TOP_N} ({pct(nonempty, TOP_N):.2f}%)")
    print(f"  Cyrillic Russian gloss: {russian}/{TOP_N} ({pct(russian, TOP_N):.2f}%)")
    print(f"  empty on every reading: {empty_words}/{TOP_N}")
    print(f"  English-only (en:, no Cyrillic): {en_only}/{TOP_N}")
    print(f"  any reading carries U+0301: {stressed_words}/{TOP_N} ({pct(stressed_words, TOP_N):.2f}%)")
    print(f"  stress among words that have Russian: {stressed_words}/{russian} ({pct(stressed_words, russian):.2f}%)")
    print(f"  polyphonic headwords: {poly}")
    print(f"  polyphonic, identical gloss on every reading: {poly_same}")
    print(f"  polyphonic, some readings empty: {poly_partial}")
    print(f"  first |-sense is a bare function word: {bare_function_words}/{TOP_N}")
    print(f"  non-empty but not Russian: {len(nonempty_not_russian)} ({', '.join(nonempty_not_russian)})")
    print(f"  gloss contains ';' (UI splits only on '|'): {semicolon_words}/{TOP_N}")
    print()
    print(f"reading coverage, rows belonging to those words: {readings}")
    print(f"  Cyrillic: {readings_ru}/{readings} ({pct(readings_ru, readings):.2f}%)")
    print(f"  empty ru: {readings_empty}")
    print(f"  with U+0301: {readings_stress}")
    print(f"senses split on '|', Cyrillic only: {senses}")
    print(f"  with U+0301: {senses_stress}/{senses} ({pct(senses_stress, senses):.2f}%)")
    print(f"  polysyllabic (2+ vowels): {poly_senses}")
    print(f"  polysyllabic with U+0301: {poly_senses_stress}/{poly_senses} ({pct(poly_senses_stress, poly_senses):.2f}%)")
    print()
    print("src on readings:")
    for src, count in src_rows.most_common():
        ru_n = src_ru[src]
        st_n = src_stress[src]
        print(f"  {src}\trows={count}\trussian={ru_n}\tstressed={st_n}")
    print()
    print("empty headwords:")
    for freq, word in empty_list:
        print(f"  rank={rank[word]} freq={freq} {word}")
    print()
    print(f"sample seed={SEED} n={SAMPLE_N} sha256={fingerprint}")
    print("sample words by rank:")
    for freq, word, _rows in sampled:
        print(f"  #{rank[word]} freq={freq} {word}")
    print()
    print("judgments:")
    for label in ("correct", "acceptable", "wrong", "garbage"):
        print(f"  {label}: {labels[label]}/{SAMPLE_N} ({pct(labels[label], SAMPLE_N):.1f}%)")
    usable = labels["correct"] + labels["acceptable"]
    print(f"  usable (correct+acceptable): {usable}/{SAMPLE_N} ({pct(usable, SAMPLE_N):.1f}%)")
    print(f"  unsure: {unsure_n}/{SAMPLE_N}")
    print()
    print(f"dict vs zh-ru on non-nogloss readings of the top {TOP_N}:")
    print(f"  same ru: {same}")
    print(f"  ru differs: {differ}")
    print(f"  only in dict.tsv: {only_dict}")

    if "--list" in sys.argv:
        print()
        for freq, word, rows in sampled:
            label, unsure, note = judged[word]
            mark = "?" if unsure else ""
            print(f"#{rank[word]}\t{word}\t{label}{mark}\t{note}")
            for row in rows:
                print(f"\t{row[3] or row[2]}\t{row[8] or '-'}\t{row[6]}")


if __name__ == "__main__":
    main()
