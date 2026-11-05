# 数据来源与署名

本项目的词典数据由以下免费数据集合成，**未使用任何付费翻译 API**。

## CC-CEDICT
https://www.mdbg.net/chinese/dictionary?page=cedict
License: Creative Commons Attribution-ShareAlike 4.0
用途：繁体/简体对照、拼音注音

## 俄语维基词典（RU Wiktionary）
https://ru.wiktionary.org/
License: Creative Commons Attribution-ShareAlike 3.0 Unported
用途：中文 → 俄语译词（直连抓取 + kaikki 英文维基中文条目经英俄枢轴）
来源：Wiktionary.org

## Wikdict 中俄词典
https://www.wikdict.com/
License: Creative Commons Attribution-ShareAlike（基于维基词典，WikDict 由 Karl Bartel 维护）
用途：中文 → 俄语译词

## FreeDict `zho-rus`
https://freedict.org/downloads/  (zho-rus 2025.11.23)
License: Creative Commons Attribution-ShareAlike 3.0 Unported
用途：中文 → 俄语译词（兜底，偏生僻）
来源：WikDict (Karl Bartel)，底层数据来自 Wiktionary.org（经 DBnary）

## Unicode Unihan 数据库
https://www.unicode.org/charts/unihan.html
License: Unicode License v3
用途：单字主读音（兜底）、生僻字英文释义（经英俄枢轴转俄语）

## jieba 词典
https://github.com/fxsjy/jieba
License: MIT
用途：候选词排序权重

## 手写释义覆盖

`data/gloss-overrides.tsv` 由本项目贡献者原创（俄语释义和中文说明），不是从上面任何一部词典摘录或转译的。
它随本仓库的数据表按 CC BY-SA 再分发：保留本文件，并以相同方式共享。

## 官方表情包和符号包

`packs/` 里的 tsv 由本项目编写：Unicode 字符（单个码位，不含 ZWJ 与变体选择符）、传统网络颜文字，以及项目自己写的拼音关键词和俄语短释义。没有从其他表情数据库整份复制。

这些文件按 CC BY-SA 4.0 随仓库分发：保留每个文件头部的说明，并以相同方式共享。它们不编进输入法 DLL。用户用脚本下载到 `%LOCALAPPDATA%\Pangyi\packs\` 之后，输入法只在本机读取。

用户自己放进该文件夹的包只留在用户的电脑上，不按上面的词典许可随本项目分发。

## 用户自己的词和释义

`%LOCALAPPDATA%\Pangyi\user-dict.tsv` 和 `user-gloss.tsv` 由用户写在自己的电脑上。输入法不会上传它们，安装包里也不包含用户填过的内容。这两份文件不是上面任何一部词典的派生数据，不按 CC BY-SA 随本项目分发。随软件发布的模板只有注释和被 `#` 注释掉的例子。

## 分发要求
CC BY-SA 要求：署名 + 以相同方式共享。若本项目再分发上述数据的衍生表，
必须保留本文件并沿用 CC BY-SA 许可。
