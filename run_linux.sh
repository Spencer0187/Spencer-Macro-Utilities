sudo su -c "" # Get root permissions for kernel to cache

wine ./visual\ studio/x64/Release/suspend.exe &
sleep 0.6
sudo ./visual\ studio/Resource\ Files/Suspend_Input_Helper_Linux_Binary suspend.exe & 

read blank

killall suspend.exe
