# Soundbox

The ESP32 creates a WPA2 Wi-Fi network named `Soundbox`. Connect a phone or
computer to it and open **http://192.168.71.1/** to adjust the volume. The
network does not provide internet access. The slider saves the setting when
released; the setting is restored from ESP32 nonvolatile storage after reboot.
The default is 10% (0.1). Volume is a linear PCM amplitude multiplier, not a
perceptually linear loudness control.

The page also sets the motion sensor re-arm delay from 0 to 600 seconds. The
delay starts when motion is detected; the sensor is re-armed when both playback
(including a failed attempt) and the delay have finished. The default is 60
seconds. Saving a new delay during the wait updates that wait, measured from the
original detection. Like volume, the delay is saved across restarts. The PIR
sensor may have its own independent hardware hold time.
Defaults apply only when no saved setting exists; previously saved values are
preserved.

The SD card file explorer lists files in the card's root directory and can
upload or delete them. Deletion is permanent and requires confirmation.
Uploads must be **mono, uncompressed PCM WAV** with 8-, 16-, 24-, or 32-bit
samples. Select an SD card filename with 1–8 ASCII letters, digits, underscores,
or hyphens followed by `.wav`; existing files are never overwritten. Invalid or
incomplete uploads are rejected. The SD card is shared with playback, so retry
file operations if playback is in progress; playback waits for an active upload
to finish. A power loss during an upload may leave an eight-hex-digit `.TMP`
file, which can be deleted from the explorer.

Before building, create a file named `.wifi-password` in the project root
containing the access point password on a single line. Use 8 to 63 printable
ASCII characters. The file is ignored by Git; the password is embedded in the
firmware, so protect the firmware binary as well. Rebuild and reflash the ESP32
after changing the password. The web page is accessible to devices connected to
the access point and uses unencrypted HTTP within that network.

The firmware uses a 1500 KiB single-app partition layout in `partitions.csv`.
`cargo run` flashes that partition table along with the firmware. When using
another flashing command, supply the same partition table and avoid erasing
the NVS partition if you want to retain saved settings.
