# Nekotatsu
Simple CLI tool that converts (specifically for) [Neko](https://github.com/CarlosEsco/Neko) backups (.proto.gz) into [Kotatsu](https://github.com/KotatsuApp/Kotatsu)'s backup format (zipped json). *There is limited support for other forks, however I cannot guarantee that they will work as intended.*

## Instructions
Note that **before you can use nekotatsu for converting**, updated lists of Tachiyomi extensions and Kotatsu parsers are necessary to map from one to the other.
Before doing anything, select a folder that you will be doing all of this work in. Once in that folder, run,
```bash
nekotatsu update
```
to automatically download and generate all the necessary files. Now you can run,
```bash
nekotatsu convert <path_to_backup>
```
to turn your backup into a zip file that Kotatsu can parse. Get this zip file on the relevant device and select Settings > Data and privacy > Restore from backup and select the zip file.

Run the commands with `--help` to view additional options.

## Motivation
First off, why specifically Neko instead of mainline Tachiyomi? No real reason honestly. It's not like I have some sort of agenda against Tachiyomi, I just use Neko specifically since I only use Mangadex, and I created this tool for my own use. (Also the name "Nekotatsu" is kinda cool.)

As for why I wanted to transfer backups to Kotatsu in the first place, I just wanted to try it out without having to go through the effort of going through my entire library one by one. The ability to sync via Mangadex is one of the useful features of Neko but I've found that it's sometimes unreliable, whether that's on the part of Neko or Mangadex, I'm not sure, but Kotatsu apparently provides a sync service that also happens to be self-hostable.

Also considering recent events around Tachiyomi, although its forks are still alive and well, I believe that having the choice to try out different options is a good thing, which is why I have done a little bit of work towards supporting non-Neko forks.

## Generating and Compiling Protocol Buffer Files
In case anyone else decides to build this manually and needs to update the Neko protobuf definitions, run this command in the repo directory:
```bash
cargo run -p tools generate <PATH_TO_KOTLIN_DEFINITIONS_DIR>
```
to generate `neko.proto`, and then run
```bash
cargo run -p tools compile
```
to create the relevant Rust source file. Before this step was included in the build script but `protoc` has proven to be difficult to wrangle for automatic builds.

## Some Known Issues
 - Sufficiently old manga on MangaDex in particular still have numerical IDs coming from Tachi forks instead of UUIDs, which messes with Kotatsu's identification procedure
 - Many sources from Tachiyomi may not be present in Kotatsu; your mileage may vary
 - Some Kotatsu parsers need additional work to be supported; I've only done enough to handle some of them properly
 - Kotatsu to Neko sucks

Although there is the possibility of me not using Kotatsu much despite making this tool, please feel free to contact me if any breaking changes to either Neko or Kotatsu causes this tool to stop functioning. Happy reading!