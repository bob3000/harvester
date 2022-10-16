# Harvester

URL block list downloader and transformer

URL block lists which list URLs leading to malicious or unwanted content can be
found all over the internet. The lists come in various formats depending on which
application they are supposed to be compatible with.

Harvester allows you the to download the lists from various sources and transform
them into a common output format.

## Features

### Output formats

- `Hostsfile`: hosts file format as found in `/etc/hosts`
    Example output:
    ```
    0.0.0.0 malicious.com
    0.0.0.0 unwanted.net
    ```
- `Lua`: a lua module returning a table
    Example output:
    ```
    return {
      "malicious.com",
      "unwanted.net",
    }
    ```

## Getting started

Harvester needs a configuration file in json format in order to work.

Example:

```json
{
  "tmp_dir": "./tmp",
  "out_dir": "./out",
  "out_format": "Lua",
  "lists": [
    {
      "id": "durablenapkin",
      "source": "https://raw.githubusercontent.com/durablenapkin/scamblocklist/master/hosts.txt",
      "tags": ["security"],
      "regex": "^0\\.0\\.0\\.0 (.*)"
    },
    {
      "id": "ut-capitole",
      "comment": "this list resides in a tar archive",
      "source": "https://dsi.ut-capitole.fr/blacklists/download/phishing.tar.gz",
      "tags": ["malware"],
      "compression": {
        "type": "TarGz",
        "archive_list_file": "phishing/domains"
      },
      "regex": "^(.*)"
    },
    {
      "id": "piholeparser",
      "source": "https://raw.githubusercontent.com/deathbybandaid/piholeparser/master/Subscribable-Lists/ParsedBlacklists/Disconnect-Malvertising-Filter.txt",
      "tags": ["security"],
      "regex": "^([^#].*)$"
    },
    {
      "id": "Spam404",
      "source": "https://raw.githubusercontent.com/Spam404/lists/master/main-blacklist.txt",
      "tags": ["security"],
      "regex": "^([^#].*)$"
    },
    {
      "id": "StevenBlack",
      "source": "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts",
      "tags": ["privacy"],
      "regex": "^0\\.0\\.0\\.0 (.*)$"
    },
    {
      "id": "RooneyMcNibNug",
      "source": "https://raw.githubusercontent.com/RooneyMcNibNug/pihole-stuff/master/SNAFU.txt",
      "tags": ["privacy"],
      "regex": "^([^#].*)$"
    },
    {
      "id": "crazy-max",
      "source": "https://raw.githubusercontent.com/crazy-max/WindowsSpyBlocker/master/data/hosts/extra.txt",
      "tags": ["microsoft", "privacy", "security"],
      "regex": "^0\\.0\\.0\\.0 (.*)"
    }
  ]
}
```

## Configuration settings

#### tmp_dir

Any writable directory to store temporary files

#### out_dir

Any writable directory to store the resulting block lists

#### out_format

The result format

#### lists

A list of block list descriptions to be downloaded

##### id

A random id which must be unique among all list ids

##### comment

An optional field to add comments to the config file

##### compression

An optional field to configure the compression used if any. Possible values are
`Gz` or `TarGz`

###### archive_list_file

If the configured compression is `TarGz` this field is needed to specify where
the list file is to be found within the archive. The value ist supposed to be a
path relative to the archive's root (e.g `tar/thelist.txt`)

##### source

The URL where the list can be downloaded

##### tags

A tag describes in which assembled category list a source list will end up

##### regex

A regular expression applied to every line of a source list to extract the URL
