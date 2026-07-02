# rpipal-assert
ATE Fixture for the [rpi-pal library](https://github.com/rpi-pal/rpi-pal)


### Behaviour 

Listens for 4bits on each of the raspberry pi's peripherals and echoes the XOR.

```math
\lambda x.\ (x \oplus \texttt{0xF})\ \&amp;\ \texttt{0xF}
```

### Supported Peripherals

- spi
- i2c
- uart
- gpio

### Unsupported Peripherals

- PWM (soon).


### Images

<img width="1000" height="750" alt="20260702_182348" src="https://github.com/user-attachments/assets/fa420694-b293-43ec-a398-5007a56f6cb3" />

### Schematic

<img width="2481" height="1720" alt="rpipal-assert" src="https://github.com/user-attachments/assets/16751aff-35c8-4d99-8e37-0565bf0a2f3e" />
