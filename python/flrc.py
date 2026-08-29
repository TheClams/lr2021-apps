'''
Basic script to parse ADS-B message from serial port coming from the adsb_rx binary
Check https://globe.adsbexchange.com for live view of aircraft
'''

# pyright: reportUnknownArgumentType=false, reportUnknownVariableType=false

import sys, re

import serial

com_ports = ['COM3', 'COM4', 'COM8', 'COM10'] # Set Serial port

def to_float(s: str) -> float:
    try:
        f = float(s)
        return f
    except:
        return 0.0

# Get Com port
ser = serial.Serial()
ser.baudrate = 576000
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

stats = {}
meas = {'rssi':[], 'ok':[], 'id':[]}
name = input("Measure: ")
capture = True
while capture:
    line = ser.readline().decode().strip()
    print(f'{line}', end='')
    if line.startswith("DONE"):
        stats[name] = meas
        nb = len(meas['rssi'])
        rssi_avg = sum(meas['rssi']) / nb
        per = sum(meas['ok']) / nb
        stats[name]['nb'] = nb
        stats[name]['rssi_avg'] = rssi_avg
        stats[name]['per'] = per
        print(f'Captured {nb} data: RSSI = {rssi_avg:.2f}, PER={per}')
        meas = {'rssi':[], 'ok':[], 'id':[]}
        name = input("Next: ")
        if name == '':
            capture = False
    else :
        m = re.match(r"\s*(?P<id>\d+) (?P<ok>\w+) -(?P<rssi>\d+)dBm", line)
        if m :
            print(f'{m.groups()}')
            meas['rssi'].append(int(m.group('rssi')))
            meas['ok'].append(1 if m.group('ok')=='OK' else 0)
            meas['id'].append(int(m.group('id')))

        else :
            print(f'Unable to parse {line}')

keys=list(stats.keys())
print(f'Saving stats to result.json ({len(keys)})')
import json
with open('result.json', 'w') as fp:
    json.dump(stats, fp, indent=4)