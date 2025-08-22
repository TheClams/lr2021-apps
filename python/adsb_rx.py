'''
Basic script to parse ADS-B message from serial port coming from the adsb_rx binary
Check https://globe.adsbexchange.com for live view of aircraft
'''
import sys
import time
import pyModeS
from pyModeS.streamer.decode import Decode

import serial

latlon = [0.0, 0.0] # Enter your position
com_ports = ['COM3', 'COM4', 'COM8', 'COM10'] # Set Serial port

adsb_msg : list[str] = []
adsb_ts  : list[float] = []
commb_msg : list[str] = []
commb_ts  : list[float] = []

def handle_messages(msg: str, t: float): # Check format of messages used in source.py

    df = pyModeS.df(msg)

    if df == 17 or df == 18:
        adsb_msg.append(msg)
        adsb_ts.append(t)
        if len(adsb_msg) > 16 :
            del adsb_msg[0]
            del adsb_ts[0]
    elif df == 20 or df == 21:
        commb_msg.append(msg)
        commb_ts.append(t)
        if len(commb_msg) > 16 :
            del commb_msg[0]
            del commb_ts[0]

def to_float(s) -> float:
    try:
        f = float(s)
        return f
    except:
        return 0.0


# Get Com port
ser = serial.Serial()
ser.baudrate = 115200
for port in com_ports:
    ser.port = port
    try :
        ser.open()
        print(f'Listening on {ser.port}')
        break
    except:
        continue
if not ser.is_open:
    sys.exit('Unable to open an UART')

decode = Decode(latlon)

while True:
    line = ser.readline().decode()
    data = line.strip().split(" | ", 2)
    if len(data) == 2:
        msg = data[0]
        rssi = data[1]
        handle_messages(msg, time.time())
        decode.process_raw(adsb_ts=adsb_ts, adsb_msg=adsb_msg, commb_ts=commb_ts, commb_msg=commb_msg)
        acs = decode.get_aircraft()
        print(f'{msg} | {rssi} : ')
        for k,v in acs.items():
            lat = to_float(v.get('lat', 0.0))
            lon = to_float(v.get('lon', 0.0))
            alt = to_float(v.get('alt', 0))
            callsign = v.get('call', '')
            print(f'  - {callsign} ({k}) : lat = {lat:.4f}, lon = {lon:.4f}, alt = {alt}')
    else :
        print(f'{data}')
