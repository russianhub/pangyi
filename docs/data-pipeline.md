# 数据流水线

本文只根据仓库里的脚本、`NOTICE.md` 和构建代码来写。脚本没有写明的下载地址、许可或生成步骤，记为**未知**，不补猜。释义质量见 [gloss-audit.md](gloss-audit.md)。

编进输入法的是 `ime/src/` 下的几份 TSV，不是 `data/raw/`。`data/raw/` 除了 `manual-zh-ru.tsv` 以外都在 `.gitignore` 里，克隆仓库拿不到原始语料。

## 仓库里现在有什么

| 文件 | 行数（含表头） | 谁读它 |
| --- | ---: | --- |
| `ime/src/dict.tsv` | 457,139（README 说的 457,138 条是不含表头） | `include_str!`，输入法词典 |
| `ime/src/abbr-keys.tsv` | 597,114 | `include_str!`，缩写键 → `dict.tsv` 行号 |
| `ime/src/lm.tsv` | 1,099,389 | `include_str!`，词 bigram |
| `ime/src/lm_uni.tsv` | 288,415 | `include_str!`，词频 |
| `ime/src/emoji.tsv` | 669 | `include_str!`，表情 |
| `data/zh-ru.tsv` | 261,969 | `build_dict.py` 的写出目标；原型和旧版训练脚本也读它 |
| `data/raw/manual-zh-ru.tsv` | 2,663 | `build_dict.py` 的人工释义，优先级最高 |

`dict.tsv` 和 `zh-ru.tsv` 表头相同：

```text
simplified  traditional  pinyin_ascii  pinyin_display  pos  freq  ru  initials  src
```

`ru` 里多个义项用 ` | ` 分隔。`src` 是这条释义从哪一路来的。

## 许可

以 `NOTICE.md` 和脚本文件头为准。

| 来源 | 脚本里写的用途 | 许可（仓库里怎么写的） | 仓库给出的入口 |
| --- | --- | --- | --- |
| CC-CEDICT | 简繁、拼音，一词一音一条 | CC BY-SA 4.0 | <https://www.mdbg.net/chinese/dictionary?page=cedict> |
| 俄语维基词典 | 中文 → 俄语，按读音 | CC BY-SA 3.0 | <https://ru.wiktionary.org/> |
| Wikdict 中俄 | 中文 → 俄语 | 脚本头写 CC-BY-SA 4.0；NOTICE 写 CC BY-SA（基于维基词典，Karl Bartel 维护） | 脚本给出 sqlite 下载地址，见下 |
| Wikdict 英俄 | 枢轴的英→俄一侧 | 脚本头写 CC-BY-SA 4.0 | 同上 |
| FreeDict `zho-rus` | 繁体 → 俄语，兜底 | CC BY-SA 3.0。NOTICE 写底层来自 Wikdict / DBnary / Wiktionary，版本号写的是 2025.11.23 | <https://freedict.org/downloads/> |
| FreeDict `eng-rus` | 枢轴的英→俄兜底 | **未知**。NOTICE 没有单列这个数据集 | 脚本只写死路径 `data/raw/eng-rus/eng-rus.tei` |
| Unicode Unihan | 单字主读音；生僻字英文释义再枢轴 | Unicode License v3 | <https://www.unicode.org/charts/unihan.html> |
| jieba | 词频，用来排序 | MIT | <https://github.com/fxsjy/jieba> |
| `manual-zh-ru.tsv` | 高频词人工释义 | **未知**。文件头写「由助手人工撰写」；没有单独许可证 | 已在仓库内 |
| 中文维基等 XML dump | 语言模型语料 | **未知**。脚本没有写 dump 的许可 | 见下文，文件名也不固定 |
| kaikki | NOTICE 和 README 写「英文维基中文条目经英俄枢轴」 | NOTICE 把它算在俄语维基词典那一节（CC BY-SA 3.0） | **未知**。`tools/` 里没有 kaikki 的文件名或下载地址 |
| `emoji.tsv` | 表情候选 | **未知** | 没有生成脚本 |

NOTICE 的分发要求：再分发这些数据的衍生表时，保留 `NOTICE.md`，并沿用 CC BY-SA。

## 每个脚本写出什么

路径都相对于仓库根。`build_dict.py`、`pivot_eng_ru.py`、`fetch_*.py`、`train_lm*.py` 用 `os.path`，从哪一级目录运行都可以。两个 Wikdict 脚本用的是字面量 `data\raw`（反斜杠），要在仓库根、并且按 Windows 路径语义来跑。

| 脚本 | 读 | 写 |
| --- | --- | --- |
| `tools/wikdict_en_ru.py` | `data/raw/wikdict-en-ru.sqlite3` | `data/raw/wikdict-en-ru.tsv` |
| `tools/wikdict_zh_ru.py` | 上面那种 sqlite 的中俄库、`cedict.txt.gz`、`tools/pylibs3` 里的 `pypinyin` | `data/raw/wikdict-zh-ru.tsv` |
| `tools/pivot_eng_ru.py` | `cedict.txt.gz`、Wikdict 英俄 TSV、`eng-rus.tei`、可选 `Unihan.zip` | `data/raw/pivot-zh-ru.tsv`，以及 `pivot-unihan-zh-ru.tsv` |
| `tools/pivot_eng_ru.py --ext` | 同上，匹配更宽（会拆英文词） | `pivot-zh-ru-ext.tsv`、`pivot-unihan-zh-ru-ext.tsv` |
| `tools/fetch_ruwikt.py [N] [chars]` | 俄语维基 API；词表来自 `jieba_dict.txt`。默认 N=5000 | 追加 `data/raw/ruwikt-zh-ru.tsv`，并写 `ruwikt-tried.txt` |
| `tools/fetch_ruwikt_cat.py enum` | 俄语维基分类 | 只打印数量 |
| `tools/fetch_ruwikt_cat.py fetch N` | 同上 | 追加到同一个 `ruwikt-zh-ru.tsv`，进度在 `ruwikt-cat-done.txt` |
| `tools/build_dict.py` | 下面「合并顺序」那一节的全部输入 | **`data/zh-ru.tsv`**。不写 `ime/src/dict.tsv` |
| `tools/train_lm.py` | `data/raw/*.xml.bz2`，用 `zh-ru.tsv` 做词表 | 覆盖 `ime/src/lm.tsv`（出现次数 ≥ 2，最多 20 万条）和 `lm_uni.tsv`（最多 6 万） |
| `tools/train_lm2.py` | 同样的 `*.xml.bz2`；词表是 `zh-ru.tsv` ∪ `dict.tsv`。默认只吃解压后的前 1.5 GB | 覆盖同一对文件。bigram：每词最多 40 条、次数 ≥ 3、全局最多 100 万。unigram：次数 ≥ 2、最多 30 万。写之前把旧文件复制为 `*.bak-414`（该后缀被 gitignore） |
| `tools/fetch_corpus.py` | 中文维基 API | `data/raw/corpus.txt`。**两个训练脚本都不读这个文件** |
| `tools/_dl_more.ps1` | 网络 | 写到脚本里的绝对路径（一台 Windows 用户目录下的 `data\raw`），只下 zhwikibooks 和 zhwikiversity 的 latest dump。不写仓库里的 `data/raw`，也不下中文维基主库 |
| `ime` 里的 `gen_abbr` | 编译进库的 `dict.tsv` | 调用者指定的路径，文档写的是 `src/abbr-keys.tsv` |
| `tools/make_ico.py` | `ime/build/icon/icon-*.png` | `ime/payload/pangyi.ico`（gitignore）。跟词典无关 |
| `tools/coverage.py`、`tools/_list_missing.py`、`tools/demo.py`、`tools/proto.py` | 原始数据或 `zh-ru.tsv` | 覆盖率脚本只打印；`_list_missing.py` 会写 `data/raw/_missing.txt`。都不产生发布用的词典 |

`emoji.tsv` 没有对应的生成脚本。文件头写了约束（全拼或缩写、不用 ZWJ、不带 VS16）。怎么扩表：**未知**，目前就是手改这份 TSV。

## 合并顺序

`build_dict.py` 的注释和代码是一致的，优先级从低到高大致是：

1. 用 CC-CEDICT 建骨架：同一个简体、同一套不带调的拼音只留一条。声调写法尽量跟 Unihan 的 `kMandarin` 对齐。
2. FreeDict `zho-rus` 按繁体词头填俄语。
3. `ruwikt-zh-ru.tsv` 盖上去。带了 `pint` 就只贴这一个读音；没有 `pint` 时贴到这个词已有的每个读音。多音字张冠李戴就是这条路径来的，见抽查文档。
4. 读入 `manual-zh-ru.tsv`。先给「词典里还没有的 (词, 拼音)」建行；已经有俄语的行先不动。
5. `pivot-zh-ru.tsv` 只填仍然为空的格子，并且只给多字词、以及 CEDICT 里有多个读音的单字。注释里写过：给常用单字也放宽时，出现过「的→яркий、开→кипеть、喝→крик」，所以收回了。
6. 保守枢轴没有的多字词，再用 `pivot-zh-ru-ext.tsv`。扩展表会丢掉含 `чего-л`、`см.`、`+` 这类词典腔的译文。
7. `pivot-unihan-zh-ru.tsv` 和它的 `-ext` 版：只补 CEDICT 里没有的单字。
8. 还是空的，填 `en: ` 加 CC-CEDICT 英文首义。多音字的单字不做这一步。
9. `wikdict-zh-ru.tsv` 再填仍为空、且只有一个读音的词；维基词典里有、骨架里完全没有的词会新建一行（`wikdict-new`）。拼音是导出时 `pypinyin` 生成的。
10. CEDICT 写着 `variant of X` 的，抄正字的俄语。
11. 来源还是 `english` 的，如果 Wikdict 有这个词，换成俄语（`wikdict-up`）。代码只升级 `english`，不改 pivot。
12. **人工表最后再盖一次。** 同一个 (简体, 拼音) 以人工表为准。注释里举过被人工表压掉的错译：他→другие、很→непослушание、看→думать、会→собрание。

写出时丢掉 `ru` 仍为空的行。词频来自 `jieba_dict.txt`，再乘 `FREQ_BOOST`：帮 ×8、给 ×2、让 ×2、请 ×1.5、邦 ×0.15、浜 ×0.05。排序是词频降序、词长、拼音。

`ruwikt` 那段在 `pint` 为空时会使用变量 `pa`。`pa` 只在 `pint` 非空时赋值。第一条这类记录会在运行时触发未赋值，后面的记录会沿用上一条的 `pa`。这是读代码能看见的行为，本仓库没有保存当时的运行日志，所以现有 `zh-ru.tsv` 是不是在这个分支上生成的，**未知**。

## 建议的运行顺序

原始文件都放进 `data/raw/`。除人工表外，这些文件不会进 git。

1. 准备 CC-CEDICT，脚本要的文件名是 `data/raw/cedict.txt.gz`。NOTICE 给的是项目页，gzip 的直链和要不要改名，**未知**。
2. FreeDict：`data/raw/zho-rus/zho-rus.tei`、`data/raw/eng-rus/eng-rus.tei`。zho-rus 的版本号 NOTICE 写了 2025.11.23；eng-rus 的版本和许可 **未知**。
3. `data/raw/Unihan.zip`，里面要有 `Unihan_Readings.txt`。NOTICE 给的是说明页，zip 直链 **未知**。
4. `data/raw/jieba_dict.txt`。NOTICE 指向 jieba 仓库；脚本没有写要把仓库里的哪个文件拷成这个名字。**未知**（jieba 发行版里通常有一份 `dict.txt`，但脚本自己没写）。
5. 下载 Wikdict sqlite 后导出：
   - <https://download.wikdict.com/dictionaries/wdweb/en-ru.sqlite3> → `data/raw/wikdict-en-ru.sqlite3`，再 `python tools/wikdict_en_ru.py`
   - <https://download.wikdict.com/dictionaries/wdweb/zh-ru.sqlite3> → `data/raw/wikdict-zh-ru.sqlite3`，再 `python tools/wikdict_zh_ru.py`
   - 中俄这一步依赖 `pypinyin`，脚本把它插在 `tools/pylibs3`。该目录被 gitignore，仓库里没有。怎么装进去，**未知**。
6. `python tools/pivot_eng_ru.py`，然后 `python tools/pivot_eng_ru.py --ext`。
7. 抓俄语维基（要联网，页面会变，不能得到和现在一模一样的 TSV）：
   - `python tools/fetch_ruwikt.py 5000` 按 jieba 词频抓缺的词
   - 或 `python tools/fetch_ruwikt_cat.py fetch 6000` 按分类抓，追加到同一个 TSV
8. 人工表不用下载，就在 `data/raw/manual-zh-ru.tsv`。
9. `python tools/build_dict.py` → `data/zh-ru.tsv`。

到这里为止，**还没有** `ime/src/dict.tsv`。见下一节。

语言模型是另一条线，不参与释义：

1. 把中文维基（或脚本能认的任何 `*.xml.bz2` 维基导出）放进 `data/raw/`。具体是哪一份 dump、哪一天的镜像，**未知**。`_dl_more.ps1` 下的两份书和学院 dump 不够解释当前的 `lm.tsv`。
2. `python tools/train_lm2.py` 或 `python tools/train_lm.py`。两者都覆盖 `ime/src/lm.tsv` 和 `lm_uni.tsv`，截断规则不同，输出不会一样。

`fetch_corpus.py` 可以单独跑，它不给训练脚本供料。

## `dict.tsv` 这一步

`ime/src/dict.rs` 写着：词典正文只能往后面加，不能整表重建，并指向一份「存档技术笔记」。仓库里没有这份笔记，**未知**它写了什么。

能确定的只有：

- `build_dict.py` 的写出路径是 `data/zh-ru.tsv`。全仓库没有第二个脚本把 `zh-ru.tsv` 拷成 `ime/src/dict.tsv`。
- `build_dict.py` 不写 `ru` 为空的行。`dict.tsv` 里有 `src=nogloss` 的空释义行（抽查时，前 5,000 词里有 73 条这样的读音）。`nogloss` 这个字符串在 `tools/*.py` 里没有出现，只在 Rust 注释里被解释成「jieba 扩容的无释义行」。生成这些行的程序 **未知**。
- 当前两份表对不上号。`dict.tsv` 有 454,385 个简体词头；`zh-ru.tsv` 有 261,968 条数据行、252,647 个简体词头。前 5,000 高频词的非 `nogloss` 读音里，5,094 条俄语相同，26 条不同，11 条只在 `dict.tsv`（`tools/audit_glosses.py` 会打印这三项）。
- 因此：按上面的顺序重跑 Python，**不能**得到现在这份 `dict.tsv`。缺的原始数据和「从 zh-ru 到 dict」的那一步都是未知。

`abbr-keys.tsv` 可以再生，但必须对着**当前这份** `dict.tsv`：

```text
cd ime
cargo run --release --bin gen_abbr -- src/abbr-keys.tsv
```

每一行是「缩写键 TAB 行号」。行号是 `dict.tsv` 里去掉表头后的行号。`dict.rs` 写明：往 `dict.tsv` 追加词条之后要重跑，否则新词没有缩写键。只改某一行的俄语、不增删行，行号可以不动。

## 怎样编进二进制

词典不走运行时文件。编译期宏把 TSV 当成字符串嵌进去：

| Rust | 宏 |
| --- | --- |
| `ime/src/dict.rs` | `include_str!("dict.tsv")`、`include_str!("abbr-keys.tsv")`、`include_str!("lm.tsv")`、`include_str!("lm_uni.tsv")` |
| `ime/src/emoji.rs` | `include_str!("emoji.tsv")` |

`ime/build.rs` **不**嵌这些表。它只在 `ime/payload/pangyi.ico` 存在时，用 windres 把图标和版本信息资源链进 `setup` 这个可执行文件。缺图标时它警告一声然后继续，DLL 照样能编。

改完 TSV 之后重新编库，新字符串才会进 DLL：

```text
cd ime
cargo build --release
```

CI（`.github/workflows/ci.yml`）在 `windows-latest` 上编的就是 git 里的 `dict.tsv` / `lm.tsv`，不跑 Python 流水线。它检查 `dict.tsv` 大于 30MB，而且不是 Git LFS 指针。发布工作流同样直接用仓库里的表打安装包。

数据表没有单独的「安装时再下载」这一步。用户机器上没有 `dict.tsv`。

## 现在这份产物里，对不上脚本的地方

这些都是对照当前文件和当前脚本得出的，不是猜测「大概是某次手工改的」：

- **`ime/src/dict.tsv` 的生成命令未知。** 见上一节。
- **`nogloss` 行的生成命令未知。**
- **kaikki 未知。** README、NOTICE 写了；`tools/` 里没有。现有枢轴是 CC-CEDICT 英文 + Wikdict/FreeDict 英俄，不是 kaikki dump。
- **`lm.tsv` 不是当前两个训练脚本的默认输出。** `train_lm.py` 最多留 20 万条 bigram，`train_lm2.py` 最多留 100 万条。仓库里的 `lm.tsv` 有 1,099,389 行，没有表头。哪一次命令、哪一份 dump 写出了它，**未知**。`lm_uni.tsv` 的 288,415 行落在 `train_lm2.py`「最多 30 万、次数 ≥ 2」的范围里，但这证明不了它就是那个脚本写的，因为 bigram 文件已经对不上。
- **`fetch_corpus.py` 的 `corpus.txt` 没有下游。**
- **`_dl_more.ps1` 不是主库的下载脚本**，路径也不在本仓库。
- **Wikdict 导出脚本在非 Windows 上对不上 `data/raw`**，因为路径写成了 `data\raw`。`pylibs3` 不在仓库里。
- **俄语维基抓取不可复现。** 在线 API、限速、词条会改。`fetch_ruwikt.py` 还依赖不在仓库里的 `jieba_dict.txt`。
- **CC-CEDICT、Unihan、jieba、FreeDict eng-rus 的精确文件 URL 或版本，脚本没写。** 上面表格里标了未知的，就是没写。
- **`emoji.tsv` 的生成方式未知**（没有脚本）。
- **人工表的许可证未知。**

重跑 `build_dict.py` 会覆盖 `data/zh-ru.tsv`。在原始数据不齐的时候不要跑。它也不会更新输入法正在用的 `dict.tsv`。
