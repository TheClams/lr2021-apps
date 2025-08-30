# LR2021 with Embassy

This repo is a series of experiment using Embassy and building a driver for the LR2021 chip from Semtech.

It is associated with a few [blog posts](https://theclams.github.io/).

## Basic Demos
 - `blinky_push`: Basic blink with speed based on button state
 - `blinky_mode`: blink example with 3 blinking speed changed on button press
 - `get_version`: first trial accessing the LR2021 chip, reading its version number
 - `get_temp`: simple application using the LR2021 temperature sensor

## LoRa

The `lora_txrx` demonstrate a simple packet TX/RX between 2 boards using LoRa (SF5/BW1000):
 * long press allow to change the board role (TX or RX)
 * single press in TX sends a packet
 * single press in RX show some stats

## FSK

The `fsk_txrx` demonstrate a simple packet TX/RX between 2 boards in GFSK:
 * long press allow to change the board role (TX or RX)
 * single press in TX sends a packet
 * single press in RX show some stats
 * Double press in TX toggle auto TX mode. When auto TX is enable the board send a packet every 250ms

The application can also be controlled through UART:
 * 's' to switch role (TX/RX)
 * 'a' to toggle auto mode
 * 't' to transmit a packet
 * 'h' to change the modulation index

## FLRC

The `flrc_txrx` demonstrate a simple packet TX/RX between 2 boards. The RX is configure to accept packet with 3 different syncwords.
 * long press allow to change the board role (TX or RX)
 * single press in TX sends a packet
 * double press in TX change the syncword (iterate over 3 predefine value)
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

## ADS-B (OOK modulation)
The `adsb_rx` application stream valid ADS-B message (i.e. CRC OK) on the UART, and a python script allows to display basic information (callsign, position, ...).

The application controls LED on the LR2021 module and flash red on CRC error and green on CRC valid.
Three action are possible through the user button:
 - a double press switch the RF channel between High level (1090MHz) and low level (978MHz)
 - a single press show RX statistics and clean them
 - a long press measure ambiant RSSI and adjust the detector threshold

## RSSI
The `rssi` application does an RSSI measurement between 400 and 1100 MHz in step of 250kHz and stream result on the UART.
The companion python script allow to display the whole spectrum as it is being measured, and can send command to the board:
 - "r[min]-[max] set the range in MHz
 - "s[step]: set the frequency step in kHz
 - "stop": stop eand exit the script

## ZWave
The demo implements some very basic sniffing capability (on the debug port only) as-well as a rough join network procedure.
A double push allow switching between the two modes.
