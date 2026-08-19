using System;
using System.Collections.Generic;
using System.Diagnostics.CodeAnalysis;
using System.Net;
using System.Net.Sockets;
using System.Net.NetworkInformation;

namespace LiteNetLib
{
    /// <summary>
    /// Address type that you want to receive from NetUtils.GetLocalIp method
    /// </summary>
    [Flags]
    public enum LocalAddrType
    {
        IPv4 = 1,
        IPv6 = 2,
        All = IPv4 | IPv6
    }

    /// <summary>
    /// Some specific network utilities
    /// </summary>
    public static class NetUtils
    {
        private static readonly NetworkSorter NetworkSorter = new();

        public static IPEndPoint MakeEndPoint(string hostStr, int port)
        {
            return new(ResolveAddress(hostStr), port);
        }

        public static IPAddress ResolveAddress(string hostStr)
        {
            if (hostStr == "localhost")
                return IPAddress.Loopback;

            if (!IPAddress.TryParse(hostStr, out IPAddress ipAddress))
            {
                if (NetManager.IPv6Support)
                    ipAddress = ResolveAddress(hostStr, AddressFamily.InterNetworkV6);
                if (ipAddress == null)
                    ipAddress = ResolveAddress(hostStr, AddressFamily.InterNetwork);
            }
            if (ipAddress == null)
                throw new ArgumentException("Invalid address: " + hostStr);

            return ipAddress;
        }

        public static IPAddress ResolveAddress(string hostStr, AddressFamily addressFamily)
        {
            IPAddress[] addresses = Dns.GetHostEntry(hostStr).AddressList;
            foreach (IPAddress ip in addresses)
            {
                if (ip.AddressFamily == addressFamily)
                {
                    return ip;
                }
            }
            return null;
        }

        /// <summary>
        /// Get all local ip addresses
        /// </summary>
        /// <param name = "addrType">type of address (IPv4, IPv6 or both)</param>
        /// <returns>List with all local ip addresses</returns>
        public static List<string> GetLocalIpList(LocalAddrType addrType)
        {
            List<string> targetList = new();
            GetLocalIpList(targetList, addrType);
            return targetList;
        }

        /// <summary>
        /// Get all local ip addresses (non alloc version)
        /// </summary>
        /// <param name = "targetList">result list</param>
        /// <param name = "addrType">type of address (IPv4, IPv6 or both)</param>
        public static void GetLocalIpList(IList<string> targetList, LocalAddrType addrType)
        {
            bool ipv4 = (addrType & LocalAddrType.IPv4) == LocalAddrType.IPv4;
            bool ipv6 = (addrType & LocalAddrType.IPv6) == LocalAddrType.IPv6;
            try
            {
                // Sort networks interfaces so it prefer Wifi over Cellular networks
                // Most cellulars networks seems to be incompatible with NAT Punch
                NetworkInterface[] networks = NetworkInterface.GetAllNetworkInterfaces();
                Array.Sort(networks, NetworkSorter);

                foreach (NetworkInterface ni in networks)
                {
                    // Skip loopback and disabled network interfaces
                    if (ni.NetworkInterfaceType == NetworkInterfaceType.Loopback || ni.OperationalStatus != OperationalStatus.Up)
                        continue;

                    IPInterfaceProperties ipProps = ni.GetIPProperties();

                    // Skip address without gateway
                    if (ipProps.GatewayAddresses.Count == 0)
                        continue;

                    foreach (UnicastIPAddressInformation ip in ipProps.UnicastAddresses)
                    {
                        IPAddress address = ip.Address;
                        if ((ipv4 && address.AddressFamily == AddressFamily.InterNetwork) || (ipv6 && address.AddressFamily == AddressFamily.InterNetworkV6))
                            targetList.Add(address.ToString());
                    }
                }

                // Fallback mode (unity android)
                if (targetList.Count == 0)
                {
                    IPAddress[] addresses = Dns.GetHostEntry(Dns.GetHostName()).AddressList;
                    foreach (IPAddress ip in addresses)
                    {
                        if ((ipv4 && ip.AddressFamily == AddressFamily.InterNetwork) || (ipv6 && ip.AddressFamily == AddressFamily.InterNetworkV6))
                            targetList.Add(ip.ToString());
                    }
                }
            }
            catch
            {
                // ignored
            }

            if (targetList.Count == 0)
            {
                if (ipv4)
                    targetList.Add("127.0.0.1");
                if (ipv6)
                    targetList.Add("::1");
            }
        }

        private static readonly List<string> IpList = new();

        /// <summary>
        /// Get first detected local ip address
        /// </summary>
        /// <param name = "addrType">type of address (IPv4, IPv6 or both)</param>
        /// <returns>IP address if available. Else - string.Empty</returns>
        public static string GetLocalIp(LocalAddrType addrType)
        {
            lock (IpList)
            {
                IpList.Clear();
                GetLocalIpList(IpList, addrType);
                return IpList.Count == 0 ? string.Empty : IpList[0];
            }
        }

        // ===========================================
        // Internal and debug log related stuff
        // ===========================================
        internal static void PrintInterfaceInfos()
        {
            NetDebug.WriteForce(NetLogLevel.Info, $"IPv6Support: {NetManager.IPv6Support}");
            try
            {
                foreach (NetworkInterface ni in NetworkInterface.GetAllNetworkInterfaces())
                {
                    foreach (UnicastIPAddressInformation ip in ni.GetIPProperties().UnicastAddresses)
                    {
                        if (ip.Address.AddressFamily == AddressFamily.InterNetwork || ip.Address.AddressFamily == AddressFamily.InterNetworkV6)
                        {
                            NetDebug.WriteForce(NetLogLevel.Info, $"Interface: {ni.Name}, Type: {ni.NetworkInterfaceType}, Ip: {ip.Address}, OpStatus: {ni.OperationalStatus}");
                        }
                    }
                }
            }
            catch (Exception e)
            {
                NetDebug.WriteForce(NetLogLevel.Info, $"Error while getting interface infos: {e}");
            }
        }

        internal static int RelativeSequenceNumber(int number, int expected)
        {
            return (number - expected + NetConstants.MaxSequence + NetConstants.HalfMaxSequence) % NetConstants.MaxSequence - NetConstants.HalfMaxSequence;
        }

        internal static T[] AllocatePinnedUninitializedArray<T>(int count) where T : unmanaged
        {
#if NET5_0_OR_GREATER || NET5_0
            return GC.AllocateUninitializedArray<T>(count, true);
#else
            return new T[count];
#endif
        }
    }

    // Pick the most obvious choice for the local IP
    // Ethernet > Wifi > Others > Cellular
    internal class NetworkSorter : IComparer<NetworkInterface>
    {
        [SuppressMessage("ReSharper", "PossibleNullReferenceException")]
        public int Compare(NetworkInterface a, NetworkInterface b)
        {
            bool isCellularA = a.NetworkInterfaceType == NetworkInterfaceType.Wman || a.NetworkInterfaceType == NetworkInterfaceType.Wwanpp || a.NetworkInterfaceType == NetworkInterfaceType.Wwanpp2;

            bool isCellularB = b.NetworkInterfaceType == NetworkInterfaceType.Wman || b.NetworkInterfaceType == NetworkInterfaceType.Wwanpp || b.NetworkInterfaceType == NetworkInterfaceType.Wwanpp2;

            bool isWifiA = a.NetworkInterfaceType == NetworkInterfaceType.Wireless80211;
            bool isWifiB = b.NetworkInterfaceType == NetworkInterfaceType.Wireless80211;

            bool isEthernetA = a.NetworkInterfaceType == NetworkInterfaceType.Ethernet || a.NetworkInterfaceType == NetworkInterfaceType.Ethernet3Megabit || a.NetworkInterfaceType == NetworkInterfaceType.GigabitEthernet || a.NetworkInterfaceType == NetworkInterfaceType.FastEthernetFx || a.NetworkInterfaceType == NetworkInterfaceType.FastEthernetT;

            bool isEthernetB = b.NetworkInterfaceType == NetworkInterfaceType.Ethernet || b.NetworkInterfaceType == NetworkInterfaceType.Ethernet3Megabit || b.NetworkInterfaceType == NetworkInterfaceType.GigabitEthernet || b.NetworkInterfaceType == NetworkInterfaceType.FastEthernetFx || b.NetworkInterfaceType == NetworkInterfaceType.FastEthernetT;

            bool isOtherA = !isCellularA && !isWifiA && !isEthernetA;
            bool isOtherB = !isCellularB && !isWifiB && !isEthernetB;

            int priorityA = isEthernetA ? 3 : isWifiA ? 2 : isOtherA ? 1 : 0;
            int priorityB = isEthernetB ? 3 : isWifiB ? 2 : isOtherB ? 1 : 0;

            return priorityA > priorityB ? -1 : priorityA < priorityB ? 1 : 0;
        }
    }
}