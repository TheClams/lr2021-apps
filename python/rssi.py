import time
import serial
import re
import threading
import math
import matplotlib.pyplot as plt
from matplotlib.ticker import MultipleLocator

# List of serial ports to try
COM_PORTS : list[str] = ['COM3', 'COM4', 'COM8', 'COM10']

stop_event = threading.Event()

def read_rssi(com_ports: list[str], rssi_dict: dict[float, float], cmd: dict[str,str]):
	ser = serial.Serial()
	# ser.baudrate = 115200
	ser.baudrate = 576000
	while not ser.is_open:
		for port in com_ports :
			try :
				ser.port = port
				ser.open()
				print(f'Port {ser.port} opened')
				break
			except:
				continue
		if not ser.is_open:
			print(f'No valid port found, retrying in 5s ...')
			time.sleep(5)
		else :
			# cnt = 0
			while not stop_event.is_set() and ser.is_open:
				try:
					line = ser.readline().decode()
					data = line.split(':')
					# cnt += 1
					# if cnt > 100:
					# 	print(f'{line}', end='')
					# 	cnt = 0
					if len(data)==2:
							rf = float(data[0]) / 1e3
							rssi = -float(data[1]) / 2.0
							rssi_dict[rf] = rssi
					if 'stop' in cmd.keys():
						return
					elif 'cmd' in cmd.keys():
						_n = ser.write(cmd['cmd'].encode())
						# print(f'[Serial] Wrote {_n} bytes : {cmd["cmd"]}')
						del cmd['cmd']

				# On serial error, consider it closed
				except serial.SerialException:
					ser.is_open = False
				# Ignore other error (likely float conversion failing due to uart)
				except:
					continue

def get_input(rssi_dict: dict[float, float], cmd_dict: dict[str,str]):
	plot_en = True
	while plot_en:
		cmd = input()
		if cmd in ('exit','stop', 'done') :
			cmd_dict['stop'] = 'now'
			plot_en = False
		else :
			cmd_dict['cmd'] = cmd
			if cmd.lower().startswith('r') :
				cmd_split = re.split(r' |:|-', cmd[1:])
				if len(cmd_split)==2:
					cmd_dict['min'] = cmd_split[0]
					cmd_dict['max'] = cmd_split[1]
			elif cmd.lower().startswith('s') :
				cmd_dict['step'] = cmd[1:]
				# Clear dictionnary when step changed
				rssi_dict.clear()
			# print(cmd_dict)


if __name__ == '__main__':
	rssi_dict : dict[float, float] = {}
	cmd_dict : dict[str,str] = {}

	cmd_dict['min'] = '400'
	cmd_dict['max'] = '1100'
	cmd_dict['step'] = '100'

	# Thread to read and plot
	read_thread = threading.Thread(target=read_rssi, args=(COM_PORTS, rssi_dict, cmd_dict))
	read_thread.start()

	input_thread = threading.Thread(target=get_input, args=(rssi_dict, cmd_dict))
	input_thread.start()


	fig, ax = plt.subplots(nrows=1, ncols=1, constrained_layout=True)
	if fig.canvas.manager is not None:
		fig.canvas.manager.set_window_title('RSSI vs Freq.')

	while True:
		if 'stop' in cmd_dict.keys() :
			read_thread.join()
			break
		ax.clear()
		freqs = list(rssi_dict.keys())
		rssi = list(rssi_dict.values())
		nb_val = min(len(freqs),len(rssi))
		if nb_val > 1 :
			ax.scatter(freqs[:nb_val], rssi[:nb_val], s=2)
			try:
				xmin = float(cmd_dict['min'])
				xmax = float(cmd_dict['max'])
				xstep = float(cmd_dict['step'])
			except:
				xmin = 400
				xmax = 1100
				xstep = 100
			delta = xmax - xmin
			if delta > 200:
				loc = (100,10)
			elif delta > 40:
				loc = (10,2)
			elif delta > 10:
				loc = (2,0.5)
			else :
				loc = (1,0.1)
			ax.set_xlim(xmin,xmax)
			ax.xaxis.set_major_locator(MultipleLocator(loc[0]))
			ax.xaxis.set_minor_locator(MultipleLocator(loc[1]))
			ax.yaxis.set_major_locator(MultipleLocator(10))
			ax.yaxis.set_minor_locator(MultipleLocator(2))
			ax.set_xlabel('Frequency (MHz)')
			f = -174 + 10*math.log10(xstep*1000)
			ax.set_ylim(math.floor(f/10)*10-5, math.ceil(f/10+7)*10)
			ax.set_ylabel('RSSI (dBm)')
			ax.grid(True, which='major', axis='both')
			ax.grid(True, which='minor', axis='both', linestyle=':')
		try:
			plt.pause(0.05)
		except:
			continue


