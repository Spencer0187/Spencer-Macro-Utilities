wine ./visual\ studio/x64/Release/suspend.exe &
sleep 0.5
sudo ./visual\ studio/Resource\ Files/Suspend_Input_Helper_Linux_Binary suspend.exe & 

read blank

killall suspend.exe
