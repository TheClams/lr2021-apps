# LR2021 with Embassy

This repo is a series of experiment using Embassy and building a driver for the LR2021 chip from Semtech.

It is associated with a few [blog posts](https://theclams.github.io/).

## Basic Demos
 - `blinky_push`: Basic blink with speed based on button state
 - `blinky_mode`: blink example with 3 blinking speed changed on button press
 - `get_version`: first trial accessing the LR2021 chip, reading its version number
 - `get_temp`: simple application using the LR2021 temperature sensor

## LoRa

The `lora_txrx` demonstrate a simple packet TX/RX between 2 boards:
 * long press allow to change the board role (TX or RX)
 * single press in TX sends a packet
 * single press in RX show some stats

## BLE

The `ble_txrx` is a very basic BLE sniffer:
 - a double press switch the RF channel: starting with an out-of-band channel and then going to BLE advertising channel 37 to 39
 - a long press switch between RX and TX
 - a single press in TX send an advertising message followed by a RX for 10ms
 - a single press in RX toggle with auto-tx mode where the board send scan request after receiving valid advertising message

The applications keeps a list of devices address so that it only displays message from a device one.
The list is limited to 32 addresses and will overwrite the oldest one if a new address is seen.

A very basic message decoding allows to see information from the advertising message.