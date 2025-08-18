import serial
import signal
import threading
import matplotlib.pyplot as plt
from matplotlib.ticker import MultipleLocator

COM_PORT = 'COM3' # Set Serial port

stop_event = threading.Event()

def keyboard_interrupt_handler(_signal, _frame):
    print("Keyboard interrupt received. Stopping the program.")
    stop_event.set()
    exit(0)

def read_rssi(com_port: str, rssi_dict: dict[float, float]):
	ser = serial.Serial()
	# ser.baudrate = 115200
	ser.baudrate = 444444
	ser.port = com_port
	ser.open()
	print(f'COM3 opened')
	while not stop_event.is_set():
		line = ser.readline().decode()
		data = line.split(':')
		# print(f'{line}')
		if len(data)==2:
			try:
				rf = float(data[0]) / 1e6
				rssi = -float(data[1]) / 2.0
				rssi_dict[rf] = rssi
			except:
				continue

if __name__ == '__main__':
	rssi_dict : dict[float, float] = {}
	read_thread = threading.Thread(target=read_rssi, args=(COM_PORT, rssi_dict))
	read_thread.start()

	# Capture interrupt to stop cleanly the script
	_ = signal.signal(signal.SIGINT, keyboard_interrupt_handler)

	fig, ax = plt.subplots(nrows=1, ncols=1, constrained_layout=True)
	fig.canvas.manager.set_window_title('RSSI vs Freq.')

	while True:
		ax.clear()
		ax.scatter(list(rssi_dict.keys()), list(rssi_dict.values()), s=2)
		ax.set_xlim(400,1100)
		ax.xaxis.set_major_locator(MultipleLocator(100))
		ax.xaxis.set_minor_locator(MultipleLocator(10))
		ax.yaxis.set_major_locator(MultipleLocator(10))
		ax.yaxis.set_minor_locator(MultipleLocator(2))
		ax.set_xlabel('Frequency (MHz)')
		ax.set_ylim(-130, -40)
		ax.set_ylabel('RSSI (dBm)')
		ax.grid(True, which='major', axis='both')
		ax.grid(True, which='minor', axis='both', linestyle=':')
		plt.pause(0.0001)
