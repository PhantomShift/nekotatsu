# Nekotatsu
Simple CLI tool that converts (specifically for) [Neko](https://github.com/CarlosEsco/Neko) backups (.proto.gz) into [Kotatsu](https://github.com/KotatsuApp/Kotatsu)'s backup format (zipped json). *There is limited support for other forks, however I cannot guarantee that they will work as intended.*

> See [nekotatsu-mobile](https://github.com/PhantomShift/nekotatsu-mobile) for a version usable on Android devices.

## Instructions
Note that **before you can use nekotatsu for converting**, updated lists of Tachiyomi extensions and Kotatsu parsers are necessary to map from one to the other.
Before attempting to do any converting, run the following command
```bash
nekotatsu update
```
to automatically download and generate all the necessary files. Note that this command downloads from the Keiyoshi repository by default; if you used a different source with your backup, make sure to override it with this command
```
nekotatsu update -t <rest of the link>.index.min.json
```
 These files will be in a relevant data directory (`.local/share/nekotatsu` on Linux and `%APPDATA%\Nekotatsu\data` on Windows (sorry mac users I don't have a mac to test on)). Now you can run,
```bash
nekotatsu convert <path_to_backup>
```
to turn your backup into a zip file that Kotatsu can parse. Get this zip file on the relevant device and select Settings > Data and privacy > Restore from backup and select the zip file.

If you don't plan on using the tool again any time soon, make sure to run `nekotatsu clear` to remove any files nekotatsu downloaded/generated from `nekotatsu update`.

## Whitelisting/Blacklisting

You may choose to filter which manga get converted with either a blacklist or a whitelist.
To make use of this, create a toml file, i.e. `nekotatsu.toml`, and populate it with an entry
named `whitelist` or `blacklist` which contains an array of source names, source URLs or source IDs.

```toml
# Example: only convert manga in your backup that came from mangadex
whitelist = [
    "mangadex"
]
```

```toml
# Example: convert all manga in your backup except for ones from this ID and URL.
blacklist = [
    2499283573021220255, # MangaDex's ID
    "www.webtoons.com"
]
```

You can then use this config by adding the `--config-file <FILE>` option, for example,

```bash
nekotatsu convert my_backup.tachibk --config-file nekotatsu.toml
```

## CLI Help

Run the commands with `--help` to view these messages.

```
Usage: nekotatsu <COMMAND>

Commands:
  convert  Convert a Neko/Tachiyomi backup into one that Kotatsu can read
  update   Downloads latest Tachiyomi source information and updates Kotatsu parser list. The resulting files are saved in the app's data directory (`~/.local/share/nekotatsu` on Linux and `%APPDATA%\Nekotatsu\data` on Windows) as `tachi_sources.json` and `kotatsu_parsers.json`
  clear    Deletes any files downloaded by nekotatsu (the data directory); Effectively the same as running `rm -rf ~/.local/share/nekotatsu` on Linux and `rmdir /s /q %APPDATA%\Nekotatsu` on Windows
  delete   Alias for `clear`
  help     Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

`convert`
```
Usage: nekotatsu convert [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Path to Neko/Tachi backup

Options:
  -o, --output <OUTPUT>
          Optional output name
      --favorites-name <FAVORITES_NAME>
          Category name for favorited manga [default: Library]
  -v, --verbose
          Display some additional information
  -V, --very-verbose
          Display all debug information; overrides verbose option
  -r, --reverse
          Convert to Neko instead
  -s, --soft-match
          Strip top-level domains when comparing Tachiyomi/Mihon sources to Kotatsu parsers
  -f, --force
          Convert without asking about overwriting existing files
  -c, --config-file <CONFIG_FILE>
```

`update`
```
Usage: nekotatsu update [OPTIONS]

Options:
  -k, --kotatsu-link <KOTATSU_LINK>  Download URL for Kotatsu parsers repo [default: https://github.com/KotatsuApp/kotatsu-parsers/archive/refs/heads/master.zip]
  -t, --tachi-link <TACHI_LINK>      Download URL for Tachiyomi extension json list (minified) [default: https://raw.githubusercontent.com/keiyoushi/extensions/repo/index.min.json]
  -f, --force-download               Force download of files even if they already exist
```

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

## Links/Credits
 - Neko - https://github.com/CarlosEsco/Neko
 - Kotatsu - https://github.com/KotatsuApp/Kotatsu
 - Keiyoushi (extensions list) - https://github.com/keiyoushi/extensions

Although there is the possibility of me not using Kotatsu much despite making this tool, please feel free to contact me if any breaking changes to either Neko or Kotatsu causes this tool to stop functioning. Happy reading!
