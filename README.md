# Roblox-Macro-Utilities
An open-source Windows C++ Roblox Macro with many features.

# Instructions (NEW UPDATE PLEASE READ)

Unzip the `.zip` and run the `.cmd` to run the program.  
Head to **[Parameters and Hotkeys](#parameters-and-hotkeys)** for Hotkey Information.
To Compile, run the project in Visual Studio 2022 and build it.

### Is this detectable???
No, it's a macro, it doesn't communicate with Roblox memory in any way.

## Roblox Anti-AFK

If you don't press `W` for fifteen minutes, a `"."` will be sent in the chat. This works even if Roblox is not the currently focused process.

## NEW Helicopter High Jump

Due to recent developments in the glitch community, we have found a way to do lag high jumps from 30-80 studs high.
This Macro Includes this, and its finally ready for release
If you offset your center of mass to the left or right, and then get your legs stuck inside of a wall like in the video below, and then activate the macro, you will fly upwards.
This works best on 120 FPS down to 60 FPS.
MUST SET SPEEDGLITCH VALUE PROPERLY!!!!

https://github.com/user-attachments/assets/fb6b8f7e-c7f3-476d-a0b3-fc99202e77ef

## Macro Speedglitch

If you use a Center of Mass offset, you can trick the engine into forcing a lot of forward momentum. (60 FPS only)

- **Non-fixed camera module:** The value for the speed glitch will be `360` divided by your Roblox sensitivity.
- **Fixed camera module:** The value will be `500` divided by your sensitivity.
- **For BOTH:** Subtract 2-3 (or more) from the final result before placing it in the .cmd.

**See [Parameters and Hotkeys](#parameters-and-hotkeys) for more information.**

## Drop-less Item Desync

Keep an item hitbox permanently on by setting it to your `"5"` slot and activating its hold hotkey for 3-5 seconds.

## Tab-glitch Remotely

You can freeze and unfreeze your game instantly by holding a button. Pretty simple, with tons of uses.

## Unequip Speedglitch

This automatically does all the inputs needed to do the Item Unequip version of the speedglitch (Only use if you cant do a normal /e dance2 speedglitch)

## Wallhop Macro

Lets you wallhop with a specific distance—self-explanatory.

## Walless Lag High Jump

By shifting your Center of Mass and using this macro, you will always perform a 14 stud LHJ (Lag High Jump).

## Press "D" for One Frame

A general script that presses a button for one frame when you press a separate button.

Inside the code is also a framework for pasting strings into chat, though it is currently unused.

# Parameters and Hotkeys

If you open the `.cmd` file inside any text editor, you will see twelve parameters, listed in this order:

1. Roblox Process Name  (Default: RobloxPlayerBeta.exe)
2. Wallhop "Strength"  (Default: `300`)
3. Speedglitch "Strength"  (Default: `960`)
4. Freeze/Tab glitch Hotkey  (Default: `Middle Mouse Button`)
5. Dropless Item Desync Hotkey  (Default: `F5` in slot 5)
6. Helicopter High Jump Hotkey (Default: `XButton1`)
7. Speedglitch Hotkey  (Default: `X`)
8. Unequip Speed Hotkey (Default: `F8`)
10. Press D Hotkey  (Default: `D`)
11. Wallhop Hotkey  (Default: `Xbutton2`)
12. Walless Lag High Jump Hotkey  (Default: `F6`)

### KEYBIND NOTES:
- **Numerical Values:** Must be integers.
- **Keybinds:** If you want to change keys, replace the `"-"` in the file with any of these options:
  - English letters (`A-Z`)
  - Function keys (`F1-F12`)
  - Alternatively, go [here](https://learn.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes) and select your key from the `"value"` column.

## Credits

- Freezing code framework based on [craftwar/suspend](https://github.com/craftwar/suspend).
