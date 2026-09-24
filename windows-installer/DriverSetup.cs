using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;

// Standard root-device installation using SetupAPI/NewDev; no bundled WDK tools.
public static class RoomWaveDriverSetup {
    [StructLayout(LayoutKind.Sequential)] struct DeviceInfo {
        public uint size; public Guid classGuid; public uint devInst; public UIntPtr reserved;
    }
    [DllImport("setupapi.dll", SetLastError=true)] static extern IntPtr SetupDiCreateDeviceInfoList(ref Guid cls, IntPtr hwnd);
    [DllImport("setupapi.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool SetupDiCreateDeviceInfoW(IntPtr set, string name, ref Guid cls, string description, IntPtr hwnd, uint flags, ref DeviceInfo data);
    [DllImport("setupapi.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool SetupDiSetDeviceRegistryPropertyW(IntPtr set, ref DeviceInfo data, uint property, byte[] buffer, uint size);
    [DllImport("setupapi.dll", SetLastError=true)] static extern bool SetupDiCallClassInstaller(uint function, IntPtr set, ref DeviceInfo data);
    [DllImport("setupapi.dll", SetLastError=true)] static extern bool SetupDiDestroyDeviceInfoList(IntPtr set);
    [DllImport("newdev.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool UpdateDriverForPlugAndPlayDevicesW(IntPtr hwnd, string hardwareId, string inf, uint flags, out bool reboot);
    static void Check(bool success) { if (!success) throw new Win32Exception(Marshal.GetLastWin32Error()); }
    public static bool Install(string inf) {
        Guid cls = new Guid("4d36e96c-e325-11ce-bfc1-08002be10318"); // MEDIA
        IntPtr set = SetupDiCreateDeviceInfoList(ref cls, IntPtr.Zero);
        if (set == new IntPtr(-1)) throw new Win32Exception(Marshal.GetLastWin32Error());
        var data = new DeviceInfo { size=(uint)Marshal.SizeOf(typeof(DeviceInfo)) };
        bool registered=false;
        try {
            Check(SetupDiCreateDeviceInfoW(set,"MEDIA",ref cls,"VB-Audio Virtual Cable",IntPtr.Zero,1,ref data));
            byte[] ids=Encoding.Unicode.GetBytes("VBAudioVACWDM\0\0");
            Check(SetupDiSetDeviceRegistryPropertyW(set,ref data,1,ids,(uint)ids.Length));
            Check(SetupDiCallClassInstaller(0x19,set,ref data));
            registered=true;
            bool reboot;
            Check(UpdateDriverForPlugAndPlayDevicesW(IntPtr.Zero,"VBAudioVACWDM",System.IO.Path.GetFullPath(inf),0,out reboot));
            return reboot;
        } catch {
            // Only this newly-created devnode; never remove a previously installed cable.
            if (registered) SetupDiCallClassInstaller(5,set,ref data);
            throw;
        } finally { SetupDiDestroyDeviceInfoList(set); }
    }
}
