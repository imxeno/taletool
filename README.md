# taletool

`taletool` is a CLI and a library for inspecting, unpacking, and packing NosTale
client data formats.

## Support

| Name                      | Contains                                           | Container support | Asset support |
| ------------------------- | -------------------------------------------------- | ----------------- | ------------- |
| `NS4BbData.NOS`           | Free-size sprite resources                         | ✅                | ✅            |
| `NScliData*.NOS`          | Client const strings                               | ✅                | ✅            |
| `NSedData.NOS`            | Effect color animation keyframes                   | ✅                | ✅            |
| `NSeffData.NOS`           | Effect definitions                                 | ✅                | ✅            |
| `NSemData.NOS`            | Effect transform animation keyframes               | ✅                | ✅            |
| `NSesData.NOS`            | Effect texture animation frame keys                | ✅                | ✅            |
| `NSetcData.NOS`           | Typewriter mini-game and unused “taboo” word lists | ✅                | ⚠️             |
| `NSgrdData*.NOS`          | Optimized map height grid data                     | ✅                | ✅            |
| `NSgtdData.NOS`           | Game data files                                    | ✅                | ⚠️             |
| `NSipData.NOS`            | Map-item sprite resources                          | ✅                | ✅            |
| `NSlangData_<locale>.NOS` | Language files                                     | ✅                | ✅            |
| `NSmcData.NOS`            | Map-object animation definitions                   | ✅                | ❌            |
| `NSmnData.NOS`            | Direct player/monster GBFC index                   | N/A               | ✅            |
| `NSmpData*.NOS`           | Map-object sprites                                 | ✅                | ✅            |
| `NSpcData.NOS`            | Player animation definitions                       | ✅                | ❌            |
| `NSpmData.NOS`            | Map-object frame/resource remap tables             | ✅                | ❌            |
| `NSpnData.NOS`            | Player/descriptor GBFC map-object index            | N/A               | ✅            |
| `NSppData*.NOS`           | Player sprites                                     | ✅                | ✅            |
| `NStcData.NOS`            | Map cell flags                                     | ✅                | ✅            |
| `NStgData*.NOS`           | Geometry                                           | ✅                | ✅            |
| `NStgeData.NOS`           | Effect geometry                                    | ✅                | ✅            |
| `NStkData.NOS`            | Map neighborhood data                              | ✅                | ⚠️             |
| `NStpData*.NOS`           | Textures                                           | ✅                | ✅            |
| `NStpeData*.NOS`          | Effect textures                                    | ✅                | ✅            |
| `NStpuData*.NOS`          | UI/widget textures                                 | ✅                | ✅            |
| `NStsData.NOS`            | Unknown and unused map-related data                | ✅                | ❌            |
| `NStuData.NOS`            | Scene bulk object-tree payloads                    | ✅                | ❌            |
| `BGM*`                    | BGM audio files                                    | N/A               | N/A           |
| `*.ntm`, `*.nam`          | Intro/act videos                                   | N/A               | N/A           |
| `snd.pck`                 | Audio files                                        | ✅                | N/A           |
| `sndinfo.lst`             | Audio metadata                                     | N/A               | ✅            |
| `*.PKG`                   | NosTale patch packages                             | ⚠️                 | ⚠️             |

✅ supported, ⚠️ partial support, ❌ not supported, N/A not applicable as an
independent container or asset layer

`*` groups the observed names for a family. Exact single, split, locale, and
locale+chunk filename patterns are listed in the format docs.

See [the docs](docs/README.md) for file format notes

## Commands

The API is still evolving so this section is a TODO once more files are
supported.

## License

Taletool is licensed under the GNU Affero General Public License version 3 or
later (`AGPL-3.0-or-later`). See [LICENSE](LICENSE).

Third-party code keeps its original license. See [NOTICE.md](NOTICE.md).
