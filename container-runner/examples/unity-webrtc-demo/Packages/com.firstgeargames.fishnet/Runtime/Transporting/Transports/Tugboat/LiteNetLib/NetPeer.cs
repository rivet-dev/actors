#if DEBUG
#define STATS_ENABLED
#endif
using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Diagnostics;
using System.Net;
using System.Threading;
using LiteNetLib.Utils;

namespace LiteNetLib
{
    /// <summary>
    /// Peer connection state
    /// </summary>
    [Flags]
    public enum ConnectionState : byte
    {
        Outgoing = 1 << 1,
        Connected = 1 << 2,
        ShutdownRequested = 1 << 3,
        Disconnected = 1 << 4,
        EndPointChange = 1 << 5,
        Any = Outgoing | Connected | ShutdownRequested | EndPointChange
    }

    internal enum ConnectRequestResult
    {
        None,
        P2PLose, // when peer connecting
        Reconnection, // when peer was connected
        NewConnection // when peer was disconnected
    }

    internal enum DisconnectResult
    {
        None,
        Reject,
        Disconnect
    }

    internal enum ShutdownResult
    {
        None,
        Success,
        WasConnected
    }

    /// <summary>
    /// Network peer. Main purpose is sending messages to specific peer.
    /// </summary>
    public class NetPeer : IPEndPoint
    {
        // Ping and RTT
        private int _rtt;
        private int _rttCount;
        private int _pingSendTimer;
        private int _rttResetTimer;
        private readonly Stopwatch _pingTimer = new();
        private int _timeSinceLastPacket;

        // Common
        private readonly object _shutdownLock = new();
        internal volatile NetPeer NextPeer;
        internal NetPeer PrevPeer;
        internal byte ConnectionNum
        {
            get => _connectNum;
            private set
            {
                _connectNum = value;
                _mergeData.ConnectionNumber = value;
                _pingPacket.ConnectionNumber = value;
                _pongPacket.ConnectionNumber = value;
            }
        }

        // Channels
        private readonly Queue<NetPacket> _unreliableChannel;
        private readonly ConcurrentQueue<BaseChannel> _channelSendQueue;
        private readonly BaseChannel[] _channels;

        // MTU
        private int _mtuIdx;
        private bool _finishMtu;
        private int _mtuCheckTimer;
        private int _mtuCheckAttempts;
        private const int MtuCheckDelay = 1000;
        private const int MaxMtuCheckAttempts = 4;
        private readonly object _mtuMutex = new();

        // Merging
        private readonly NetPacket _mergeData;
        private int _mergePos;
        private int _mergeCount;

        // Connection
        private int _connectAttempts;
        private int _connectTimer;
        private byte _connectNum;
        private NetPacket _shutdownPacket;
        private const int ShutdownDelay = 300;
        private int _shutdownTimer;
        private readonly NetPacket _pingPacket;
        private readonly NetPacket _pongPacket;
        private readonly NetPacket _connectRequestPacket;
        private readonly NetPacket _connectAcceptPacket;
        /// <summary>
        /// Peer parent NetManager
        /// </summary>
        public readonly NetManager NetManager;
        /// <summary>
        /// Current connection state
        /// </summary>
        public ConnectionState ConnectionState { get; private set; }
        /// <summary>
        /// Connection time for internal purposes
        /// </summary>
        internal long ConnectTime { get; private set; }
        /// <summary>
        /// Peer id can be used as key in your dictionary of peers
        /// </summary>
        public readonly int Id;
        /// <summary>
        /// Id assigned from server
        /// </summary>
        public int RemoteId { get; private set; }
        /// <summary>
        /// Current one-way ping (RTT/2) in milliseconds
        /// </summary>
        public int Ping => RoundTripTime / 2;
        /// <summary>
        /// Round trip time in milliseconds
        /// </summary>
        public int RoundTripTime { get; private set; }
        /// <summary>
        /// Current MTU - Maximum Transfer Unit ( maximum udp packet size without fragmentation )
        /// </summary>
        public int Mtu { get; private set; }
        /// <summary>
        /// Delta with remote time in ticks (not accurate)
        /// positive - remote time > our time
        /// </summary>
        public long RemoteTimeDelta { get; private set; }
        /// <summary>
        /// Remote UTC time (not accurate)
        /// </summary>
        public DateTime RemoteUtcTime => new(DateTime.UtcNow.Ticks + RemoteTimeDelta);
        /// <summary>
        /// Time since last packet received (including internal library packets)
        /// </summary>
        public int TimeSinceLastPacket => _timeSinceLastPacket;
        internal double ResendDelay { get; private set; } = 27.0;
        /// <summary>
        /// Application defined object containing data about the connection
        /// </summary>
        public object Tag;
        /// <summary>
        /// Statistics of peer connection
        /// </summary>
        public readonly NetStatistics Statistics;
        private SocketAddress _cachedSocketAddr;
        private int _cachedHashCode;
        internal byte[] NativeAddress;

        /// <summary>
        /// IPEndPoint serialize
        /// </summary>
        /// <returns>SocketAddress</returns>
        public override SocketAddress Serialize()
        {
            return _cachedSocketAddr;
        }

        public override int GetHashCode()
        {
            // uses SocketAddress hash in NET8 and IPEndPoint hash for NativeSockets and previous NET versions
            return _cachedHashCode;
        }

        // incoming connection constructor
        internal NetPeer(NetManager netManager, IPEndPoint remoteEndPoint, int id) : base(remoteEndPoint.Address, remoteEndPoint.Port)
        {
            Id = id;
            Statistics = new();
            NetManager = netManager;

            _cachedSocketAddr = base.Serialize();
            if (NetManager.UseNativeSockets)
            {
                NativeAddress = new byte[_cachedSocketAddr.Size];
                for (int i = 0; i < _cachedSocketAddr.Size; i++)
                    NativeAddress[i] = _cachedSocketAddr[i];
            }
#if NET8_0_OR_GREATER
            _cachedHashCode = NetManager.UseNativeSockets ? base.GetHashCode() : _cachedSocketAddr.GetHashCode();
#else
            _cachedHashCode = base.GetHashCode();
#endif

            ResetMtu();

            ConnectionState = ConnectionState.Connected;
            _mergeData = new(PacketProperty.Merged, NetConstants.MaxPacketSize);
            _pongPacket = new(PacketProperty.Pong, 0);
            _pingPacket = new(PacketProperty.Ping, 0) { Sequence = 1 };

            _unreliableChannel = new();

            _channels = new BaseChannel[netManager.ChannelsCount * NetConstants.ChannelTypeCount];
            _channelSendQueue = new();
        }

        internal void InitiateEndPointChange()
        {
            ResetMtu();
            ConnectionState = ConnectionState.EndPointChange;
        }

        internal void FinishEndPointChange(IPEndPoint newEndPoint)
        {
            if (ConnectionState != ConnectionState.EndPointChange)
                return;
            ConnectionState = ConnectionState.Connected;

            Address = newEndPoint.Address;
            Port = newEndPoint.Port;

            if (NetManager.UseNativeSockets)
            {
                NativeAddress = new byte[_cachedSocketAddr.Size];
                for (int i = 0; i < _cachedSocketAddr.Size; i++)
                    NativeAddress[i] = _cachedSocketAddr[i];
            }
            _cachedSocketAddr = base.Serialize();
#if NET8_0_OR_GREATER
            _cachedHashCode = NetManager.UseNativeSockets ? base.GetHashCode() : _cachedSocketAddr.GetHashCode();
#else
            _cachedHashCode = base.GetHashCode();
#endif
        }

        internal void ResetMtu()
        {
            _finishMtu = false;
            if (NetManager.MtuOverride > 0)
                OverrideMtu(NetManager.MtuOverride);
            else if (NetManager.UseSafeMtu)
                SetMtu(0);
            else
                SetMtu(1);
        }

        private void SetMtu(int mtuIdx)
        {
            _mtuIdx = mtuIdx;
            Mtu = NetConstants.PossibleMtu[mtuIdx] - NetManager.ExtraPacketSizeForLayer;
        }

        private void OverrideMtu(int mtuValue)
        {
            Mtu = mtuValue;
            _finishMtu = true;
        }

        /// <summary>
        /// Returns packets count in queue for reliable channel
        /// </summary>
        /// <param name = "channelNumber">number of channel 0-63</param>
        /// <param name = "ordered">type of channel ReliableOrdered or ReliableUnordered</param>
        /// <returns>packets count in channel queue</returns>
        public int GetPacketsCountInReliableQueue(byte channelNumber, bool ordered)
        {
            int idx = channelNumber * NetConstants.ChannelTypeCount + (byte)(ordered ? DeliveryMethod.ReliableOrdered : DeliveryMethod.ReliableUnordered);
            BaseChannel channel = _channels[idx];
            return channel != null ? ((ReliableChannel)channel).PacketsInQueue : 0;
        }

        /// <summary>
        /// Create temporary packet (maximum size MTU - headerSize) to send later without additional copies
        /// </summary>
        /// <param name = "deliveryMethod">Delivery method (reliable, unreliable, etc.)</param>
        /// <param name = "channelNumber">Number of channel (from 0 to channelsCount - 1)</param>
        /// <returns>PooledPacket that you can use to write data starting from UserDataOffset</returns>
        public PooledPacket CreatePacketFromPool(DeliveryMethod deliveryMethod, byte channelNumber)
        {
            // multithreaded variable
            int mtu = Mtu;
            NetPacket packet = NetManager.PoolGetPacket(mtu);
            if (deliveryMethod == DeliveryMethod.Unreliable)
            {
                packet.Property = PacketProperty.Unreliable;
                return new(packet, mtu, 0);
            }
            else
            {
                packet.Property = PacketProperty.Channeled;
                return new(packet, mtu, (byte)(channelNumber * NetConstants.ChannelTypeCount + (byte)deliveryMethod));
            }
        }

        /// <summary>
        /// Sends pooled packet without data copy
        /// </summary>
        /// <param name = "packet">packet to send</param>
        /// <param name = "userDataSize">size of user data you want to send</param>
        public void SendPooledPacket(PooledPacket packet, int userDataSize)
        {
            if (ConnectionState != ConnectionState.Connected)
                return;
            packet._packet.Size = packet.UserDataOffset + userDataSize;
            if (packet._packet.Property == PacketProperty.Channeled)
            {
                CreateChannel(packet._channelNumber).AddToQueue(packet._packet);
            }
            else
            {
                lock (_unreliableChannel)
                {
                    _unreliableChannel.Enqueue(packet._packet);
                }
            }
        }

        private BaseChannel CreateChannel(byte idx)
        {
            BaseChannel newChannel = _channels[idx];
            if (newChannel != null)
                return newChannel;
            switch ((DeliveryMethod)(idx % NetConstants.ChannelTypeCount))
            {
                case DeliveryMethod.ReliableUnordered:
                    newChannel = new ReliableChannel(this, false, idx);
                    break;
                case DeliveryMethod.Sequenced:
                    newChannel = new SequencedChannel(this, false, idx);
                    break;
                case DeliveryMethod.ReliableOrdered:
                    newChannel = new ReliableChannel(this, true, idx);
                    break;
                case DeliveryMethod.ReliableSequenced:
                    newChannel = new SequencedChannel(this, true, idx);
                    break;
            }
            BaseChannel prevChannel = Interlocked.CompareExchange(ref _channels[idx], newChannel, null);
            if (prevChannel != null)
                return prevChannel;

            return newChannel;
        }

        // "Connect to" constructor
        internal NetPeer(NetManager netManager, IPEndPoint remoteEndPoint, int id, byte connectNum, NetDataWriter connectData) : this(netManager, remoteEndPoint, id)
        {
            ConnectTime = DateTime.UtcNow.Ticks;
            ConnectionState = ConnectionState.Outgoing;
            ConnectionNum = connectNum;

            // Make initial packet
            _connectRequestPacket = NetConnectRequestPacket.Make(connectData, remoteEndPoint.Serialize(), ConnectTime, id);
            _connectRequestPacket.ConnectionNumber = connectNum;

            // Send request
            NetManager.SendRaw(_connectRequestPacket, this);

            NetDebug.Write(NetLogLevel.Trace, $"[CC] ConnectId: {ConnectTime}, ConnectNum: {connectNum}");
        }

        // "Accept" incoming constructor
        internal NetPeer(NetManager netManager, ConnectionRequest request, int id) : this(netManager, request.RemoteEndPoint, id)
        {
            ConnectTime = request.InternalPacket.ConnectionTime;
            ConnectionNum = request.InternalPacket.ConnectionNumber;
            RemoteId = request.InternalPacket.PeerId;

            // Make initial packet
            _connectAcceptPacket = NetConnectAcceptPacket.Make(ConnectTime, ConnectionNum, id);

            // Make Connected
            ConnectionState = ConnectionState.Connected;

            // Send
            NetManager.SendRaw(_connectAcceptPacket, this);

            NetDebug.Write(NetLogLevel.Trace, $"[CC] ConnectId: {ConnectTime}");
        }

        // Reject
        internal void Reject(NetConnectRequestPacket requestData, byte[] data, int start, int length)
        {
            ConnectTime = requestData.ConnectionTime;
            _connectNum = requestData.ConnectionNumber;
            Shutdown(data, start, length, false);
        }

        internal bool ProcessConnectAccept(NetConnectAcceptPacket packet)
        {
            if (ConnectionState != ConnectionState.Outgoing)
                return false;

            // check connection id
            if (packet.ConnectionTime != ConnectTime)
            {
                NetDebug.Write(NetLogLevel.Trace, $"[NC] Invalid connectId: {packet.ConnectionTime} != our({ConnectTime})");
                return false;
            }
            // check connect num
            ConnectionNum = packet.ConnectionNumber;
            RemoteId = packet.PeerId;

            NetDebug.Write(NetLogLevel.Trace, "[NC] Received connection accept");
            Interlocked.Exchange(ref _timeSinceLastPacket, 0);
            ConnectionState = ConnectionState.Connected;
            return true;
        }

        /// <summary>
        /// Gets maximum size of packet that will be not fragmented.
        /// </summary>
        /// <param name = "options">Type of packet that you want send</param>
        /// <returns>size in bytes</returns>
        public int GetMaxSinglePacketSize(DeliveryMethod options)
        {
            return Mtu - NetPacket.GetHeaderSize(options == DeliveryMethod.Unreliable ? PacketProperty.Unreliable : PacketProperty.Channeled);
        }

        /// <summary>
        /// Send data to peer with delivery event called
        /// </summary>
        /// <param name = "data">Data</param>
        /// <param name = "channelNumber">Number of channel (from 0 to channelsCount - 1)</param>
        /// <param name = "deliveryMethod">Delivery method (reliable, unreliable, etc.)</param>
        /// <param name = "userData">User data that will be received in DeliveryEvent</param>
        /// <exception cref = "ArgumentException">
        /// If you trying to send unreliable packet type
        /// <para/>
        /// </exception>
        public void SendWithDeliveryEvent(byte[] data, byte channelNumber, DeliveryMethod deliveryMethod, object userData)
        {
            if (deliveryMethod != DeliveryMethod.ReliableOrdered && deliveryMethod != DeliveryMethod.ReliableUnordered)
                throw new ArgumentException("Delivery event will work only for ReliableOrdered/Unordered packets");
            SendInternal(data, 0, data.Length, channelNumber, deliveryMethod, userData);
        }

        /// <summary>
        /// Send data to peer with delivery event called
        /// </summary>
        /// <param name = "data">Data</param>
        /// <param name = "start">Start of data</param>
        /// <param name = "length">Length of data</param>
        /// <param name = "channelNumber">Number of channel (from 0 to channelsCount - 1)</param>
        /// <param name = "deliveryMethod">Delivery method (reliable, unreliable, etc.)</param>
        /// <param name = "userData">User data that will be received in DeliveryEvent</param>
        /// <exception cref = "ArgumentException">
        /// If you trying to send unreliable packet type
        /// <para/>
        /// </exception>
        public void SendWithDeliveryEvent(byte[] data, int start, int length, byte channelNumber, DeliveryMethod deliveryMethod, object userData)
        {
            if (deliveryMethod != DeliveryMethod.ReliableOrdered && deliveryMethod != DeliveryMethod.ReliableUnordered)
                throw new ArgumentException("Delivery event will work only for ReliableOrdered/Unordered packets");
            SendInternal(data, start, length, channelNumber, deliveryMethod, userData);
        }

        /// <summary>
        /// Send data to peer with delivery event called
        /// </summary>
        /// <param name = "dataWriter">Data</param>
        /// <param name = "channelNumber">Number of channel (from 0 to channelsCount - 1)</param>
        /// <param name = "deliveryMethod">Delivery method (reliable, unreliable, etc.)</param>
        /// <param name = "userData">User data that will be received in DeliveryEvent</param>
        /// <exception cref = "ArgumentException">
        /// If you trying to send unreliable packet type
        /// <para/>
        /// </exception>
        public void SendWithDeliveryEvent(NetDataWriter dataWriter, byte channelNumber, DeliveryMethod deliveryMethod, object userData)
        {
            if (deliveryMethod != DeliveryMethod.ReliableOrdered && deliveryMethod != DeliveryMethod.ReliableUnordered)
                throw new ArgumentException("Delivery event will work only for ReliableOrdered/Unordered packets");
            SendInternal(dataWriter.Data, 0, dataWriter.Length, channelNumber, deliveryMethod, userData);
        }

        /// <summary>
        /// Send data to peer (channel - 0)
        /// </summary>
        /// <param name = "data">Data</param>
        /// <param name = "deliveryMethod">Send options (reliable, unreliable, etc.)</param>
        /// <exception cref = "TooBigPacketException">
        /// If size exceeds maximum limit:
        /// <para/>
        /// MTU - headerSize bytes for Unreliable
        /// <para/>
        /// Fragment count exceeded ushort.MaxValue
        /// <para/>
        /// </exception>
        public void Send(byte[] data, DeliveryMethod deliveryMethod)
        {
            SendInternal(data, 0, data.Length, 0, deliveryMethod, null);
        }

        /// <summary>
        /// Send data to peer (channel - 0)
        /// </summary>
        /// <param name = "dataWriter">DataWriter with data</param>
        /// <param name = "deliveryMethod">Send options (reliable, unreliable, etc.)</param>
        /// <exception cref = "TooBigPacketException">
        /// If size exceeds maximum limit:
        /// <para/>
        /// MTU - headerSize bytes for Unreliable
        /// <para/>
        /// Fragment count exceeded ushort.MaxValue
        /// <para/>
        /// </exception>
        public void Send(NetDataWriter dataWriter, DeliveryMethod deliveryMethod)
        {
            SendInternal(dataWriter.Data, 0, dataWriter.Length, 0, deliveryMethod, null);
        }

        /// <summary>
        /// Send data to peer (channel - 0)
        /// </summary>
        /// <param name = "data">Data</param>
        /// <param name = "start">Start of data</param>
        /// <param name = "length">Length of data</param>
        /// <param name = "options">Send options (reliable, unreliable, etc.)</param>
        /// <exception cref = "TooBigPacketException">
        /// If size exceeds maximum limit:
        /// <para/>
        /// MTU - headerSize bytes for Unreliable
        /// <para/>
        /// Fragment count exceeded ushort.MaxValue
        /// <para/>
        /// </exception>
        public void Send(byte[] data, int start, int length, DeliveryMethod options)
        {
            SendInternal(data, start, length, 0, options, null);
        }

        /// <summary>
        /// Send data to peer
        /// </summary>
        /// <param name = "data">Data</param>
        /// <param name = "channelNumber">Number of channel (from 0 to channelsCount - 1)</param>
        /// <param name = "deliveryMethod">Send options (reliable, unreliable, etc.)</param>
        /// <exception cref = "TooBigPacketException">
        /// If size exceeds maximum limit:
        /// <para/>
        /// MTU - headerSize bytes for Unreliable
        /// <para/>
        /// Fragment count exceeded ushort.MaxValue
        /// <para/>
        /// </exception>
        public void Send(byte[] data, byte channelNumber, DeliveryMethod deliveryMethod)
        {
            SendInternal(data, 0, data.Length, channelNumber, deliveryMethod, null);
        }

        /// <summary>
        /// Send data to peer
        /// </summary>
        /// <param name = "dataWriter">DataWriter with data</param>
        /// <param name = "channelNumber">Number of channel (from 0 to channelsCount - 1)</param>
        /// <param name = "deliveryMethod">Send options (reliable, unreliable, etc.)</param>
        /// <exception cref = "TooBigPacketException">
        /// If size exceeds maximum limit:
        /// <para/>
        /// MTU - headerSize bytes for Unreliable
        /// <para/>
        /// Fragment count exceeded ushort.MaxValue
        /// <para/>
        /// </exception>
        public void Send(NetDataWriter dataWriter, byte channelNumber, DeliveryMethod deliveryMethod)
        {
            SendInternal(dataWriter.Data, 0, dataWriter.Length, channelNumber, deliveryMethod, null);
        }

        /// <summary>
        /// Send data to peer
        /// </summary>
        /// <param name = "data">Data</param>
        /// <param name = "start">Start of data</param>
        /// <param name = "length">Length of data</param>
        /// <param name = "channelNumber">Number of channel (from 0 to channelsCount - 1)</param>
        /// <param name = "deliveryMethod">Delivery method (reliable, unreliable, etc.)</param>
        /// <exception cref = "TooBigPacketException">
        /// If size exceeds maximum limit:
        /// <para/>
        /// MTU - headerSize bytes for Unreliable
        /// <para/>
        /// Fragment count exceeded ushort.MaxValue
        /// <para/>
        /// </exception>
        public void Send(byte[] data, int start, int length, byte channelNumber, DeliveryMethod deliveryMethod)
        {
            SendInternal(data, start, length, channelNumber, deliveryMethod, null);
        }

        private void SendInternal(byte[] data, int start, int length, byte channelNumber, DeliveryMethod deliveryMethod, object userData)
        {
            if (ConnectionState != ConnectionState.Connected || channelNumber >= _channels.Length)
                return;

            // Select channel
            PacketProperty property;
            BaseChannel channel = null;

            if (deliveryMethod == DeliveryMethod.Unreliable)
            {
                property = PacketProperty.Unreliable;
            }
            else
            {
                property = PacketProperty.Channeled;
                channel = CreateChannel((byte)(channelNumber * NetConstants.ChannelTypeCount + (byte)deliveryMethod));
            }

            // Prepare
            NetDebug.Write("[RS]Packet: " + property);

            // Check fragmentation
            int headerSize = NetPacket.GetHeaderSize(property);
            // Save mtu for multithread
            int mtu = Mtu;
            if (length + headerSize > mtu)
            {
                NetDebug.WriteError($"Packet size {length + headerSize} exceeds MTU {mtu}. Fragmentation is disabled.");
                throw new TooBigPacketException($"Packet size {length + headerSize} exceeded MTU {mtu}. LNL Fragmentation was removed.");
            }

            // Else just send
            NetPacket packet = NetManager.PoolGetPacket(headerSize + length);
            packet.Property = property;
            Buffer.BlockCopy(data, start, packet.RawData, headerSize, length);
            packet.UserData = userData;

            if (channel == null) // unreliable
            {
                lock (_unreliableChannel)
                {
                    _unreliableChannel.Enqueue(packet);
                }
            }
            else
            {
                channel.AddToQueue(packet);
            }
        }

#if LITENETLIB_SPANS || NETCOREAPP2_1_OR_GREATER || NETSTANDARD2_1_OR_GREATER || NETCOREAPP2_1 || NETCOREAPP3_1 || NET5_0 || NETSTANDARD2_1
        /// <summary>
        /// Send data to peer with delivery event called
        /// </summary>
        /// <param name = "data">Data</param>
        /// <param name = "channelNumber">Number of channel (from 0 to channelsCount - 1)</param>
        /// <param name = "deliveryMethod">Delivery method (reliable, unreliable, etc.)</param>
        /// <param name = "userData">User data that will be received in DeliveryEvent</param>
        /// <exception cref = "ArgumentException">
        /// If you trying to send unreliable packet type
        /// <para/>
        /// </exception>
        public void SendWithDeliveryEvent(ReadOnlySpan<byte> data, byte channelNumber, DeliveryMethod deliveryMethod, object userData)
        {
            if (deliveryMethod != DeliveryMethod.ReliableOrdered && deliveryMethod != DeliveryMethod.ReliableUnordered)
                throw new ArgumentException("Delivery event will work only for ReliableOrdered/Unordered packets");
            SendInternal(data, channelNumber, deliveryMethod, userData);
        }

        /// <summary>
        /// Send data to peer (channel - 0)
        /// </summary>
        /// <param name = "data">Data</param>
        /// <param name = "deliveryMethod">Send options (reliable, unreliable, etc.)</param>
        /// <exception cref = "TooBigPacketException">
        /// If size exceeds maximum limit:
        /// <para/>
        /// MTU - headerSize bytes for Unreliable
        /// <para/>
        /// Fragment count exceeded ushort.MaxValue
        /// <para/>
        /// </exception>
        public void Send(ReadOnlySpan<byte> data, DeliveryMethod deliveryMethod)
        {
            SendInternal(data, 0, deliveryMethod, null);
        }

        /// <summary>
        /// Send data to peer
        /// </summary>
        /// <param name = "data">Data</param>
        /// <param name = "channelNumber">Number of channel (from 0 to channelsCount - 1)</param>
        /// <param name = "deliveryMethod">Send options (reliable, unreliable, etc.)</param>
        /// <exception cref = "TooBigPacketException">
        /// If size exceeds maximum limit:
        /// <para/>
        /// MTU - headerSize bytes for Unreliable
        /// <para/>
        /// Fragment count exceeded ushort.MaxValue
        /// <para/>
        /// </exception>
        public void Send(ReadOnlySpan<byte> data, byte channelNumber, DeliveryMethod deliveryMethod)
        {
            SendInternal(data, channelNumber, deliveryMethod, null);
        }

        private void SendInternal(ReadOnlySpan<byte> data, byte channelNumber, DeliveryMethod deliveryMethod, object userData)
        {
            if (ConnectionState != ConnectionState.Connected || channelNumber >= _channels.Length)
                return;

            // Select channel
            PacketProperty property;
            BaseChannel channel = null;

            if (deliveryMethod == DeliveryMethod.Unreliable)
            {
                property = PacketProperty.Unreliable;
            }
            else
            {
                property = PacketProperty.Channeled;
                channel = CreateChannel((byte)(channelNumber * NetConstants.ChannelTypeCount + (byte)deliveryMethod));
            }

            // Prepare
            NetDebug.Write("[RS]Packet: " + property);

            // Check fragmentation
            int headerSize = NetPacket.GetHeaderSize(property);
            // Save mtu for multithread
            int mtu = Mtu;
            int length = data.Length;
            if (length + headerSize > mtu)
            {
                NetDebug.WriteError($"Packet size {length + headerSize} exceeds MTU {mtu}. Fragmentation is disabled.");
                throw new TooBigPacketException($"Packet size {length + headerSize} exceeded MTU {mtu}. LNL Fragmentation was removed.");
            }

            // Else just send
            NetPacket packet = NetManager.PoolGetPacket(headerSize + length);
            packet.Property = property;
            data.CopyTo(new(packet.RawData, headerSize, length));
            packet.UserData = userData;

            if (channel == null) // unreliable
            {
                lock (_unreliableChannel)
                {
                    _unreliableChannel.Enqueue(packet);
                }
            }
            else
            {
                channel.AddToQueue(packet);
            }
        }
#endif

        public void Disconnect(byte[] data)
        {
            NetManager.DisconnectPeer(this, data);
        }

        public void Disconnect(NetDataWriter writer)
        {
            NetManager.DisconnectPeer(this, writer);
        }

        public void Disconnect(byte[] data, int start, int count)
        {
            NetManager.DisconnectPeer(this, data, start, count);
        }

        public void Disconnect()
        {
            NetManager.DisconnectPeer(this);
        }

        internal DisconnectResult ProcessDisconnect(NetPacket packet)
        {
            if ((ConnectionState == ConnectionState.Connected || ConnectionState == ConnectionState.Outgoing) && packet.Size >= 9 && BitConverter.ToInt64(packet.RawData, 1) == ConnectTime && packet.ConnectionNumber == _connectNum)
            {
                return ConnectionState == ConnectionState.Connected ? DisconnectResult.Disconnect : DisconnectResult.Reject;
            }
            return DisconnectResult.None;
        }

        internal void AddToReliableChannelSendQueue(BaseChannel channel)
        {
            _channelSendQueue.Enqueue(channel);
        }

        internal ShutdownResult Shutdown(byte[] data, int start, int length, bool force)
        {
            lock (_shutdownLock)
            {
                // trying to shutdown already disconnected
                if (ConnectionState == ConnectionState.Disconnected || ConnectionState == ConnectionState.ShutdownRequested)
                {
                    return ShutdownResult.None;
                }

                ShutdownResult result = ConnectionState == ConnectionState.Connected ? ShutdownResult.WasConnected : ShutdownResult.Success;

                // don't send anything
                if (force)
                {
                    ConnectionState = ConnectionState.Disconnected;
                    return result;
                }

                //reset time for reconnect protection
                Interlocked.Exchange(ref _timeSinceLastPacket, 0);

                //send shutdown packet
                _shutdownPacket = new(PacketProperty.Disconnect, length) { ConnectionNumber = _connectNum };
                FastBitConverter.GetBytes(_shutdownPacket.RawData, 1, ConnectTime);
                if (_shutdownPacket.Size >= Mtu)
                {
                    //Drop additional data
                    NetDebug.WriteError("[Peer] Disconnect additional data size more than MTU - 8!");
                }
                else if (data != null && length > 0)
                {
                    Buffer.BlockCopy(data, start, _shutdownPacket.RawData, 9, length);
                }
                ConnectionState = ConnectionState.ShutdownRequested;
                NetDebug.Write("[Peer] Send disconnect");
                NetManager.SendRaw(_shutdownPacket, this);
                return result;
            }
        }

        private void UpdateRoundTripTime(int roundTripTime)
        {
            _rtt += roundTripTime;
            _rttCount++;
            RoundTripTime = _rtt / _rttCount;
            ResendDelay = 25.0 + RoundTripTime * 2.1; // 25 ms + double rtt
        }

        internal void AddReliablePacket(DeliveryMethod method, NetPacket p)
        {
            if (p.IsFragmented)
            {
                NetDebug.WriteError("Fragmented packets are disabled. Dropping packet.");
                NetManager.PoolRecycle(p);
                return;
            }

            NetManager.CreateReceiveEvent(p, method, (byte)(p.ChannelId / NetConstants.ChannelTypeCount), NetConstants.ChanneledHeaderSize, this);
        }

        private void ProcessMtuPacket(NetPacket packet)
        {
            //header + int
            if (packet.Size < NetConstants.PossibleMtu[0])
                return;

            //first stage check (mtu check and mtu ok)
            int receivedMtu = BitConverter.ToInt32(packet.RawData, 1);
            int endMtuCheck = BitConverter.ToInt32(packet.RawData, packet.Size - 4);
            if (receivedMtu != packet.Size || receivedMtu != endMtuCheck || receivedMtu > NetConstants.MaxPacketSize)
            {
                NetDebug.WriteError($"[MTU] Broken packet. RMTU {receivedMtu}, EMTU {endMtuCheck}, PSIZE {packet.Size}");
                return;
            }

            if (packet.Property == PacketProperty.MtuCheck)
            {
                _mtuCheckAttempts = 0;
                NetDebug.Write("[MTU] check. send back: " + receivedMtu);
                packet.Property = PacketProperty.MtuOk;
                NetManager.SendRawAndRecycle(packet, this);
            }
            else if (receivedMtu > Mtu && !_finishMtu) //MtuOk
            {
                //invalid packet
                if (receivedMtu != NetConstants.PossibleMtu[_mtuIdx + 1] - NetManager.ExtraPacketSizeForLayer)
                    return;

                lock (_mtuMutex)
                {
                    SetMtu(_mtuIdx + 1);
                }
                //if maxed - finish.
                if (_mtuIdx == NetConstants.PossibleMtu.Length - 1)
                    _finishMtu = true;
                NetManager.PoolRecycle(packet);
                NetDebug.Write("[MTU] ok. Increase to: " + Mtu);
            }
        }

        private void UpdateMtuLogic(int deltaTime)
        {
            if (_finishMtu)
                return;

            _mtuCheckTimer += deltaTime;
            if (_mtuCheckTimer < MtuCheckDelay)
                return;

            _mtuCheckTimer = 0;
            _mtuCheckAttempts++;
            if (_mtuCheckAttempts >= MaxMtuCheckAttempts)
            {
                _finishMtu = true;
                return;
            }

            lock (_mtuMutex)
            {
                if (_mtuIdx >= NetConstants.PossibleMtu.Length - 1)
                    return;

                //Send increased packet
                int newMtu = NetConstants.PossibleMtu[_mtuIdx + 1] - NetManager.ExtraPacketSizeForLayer;
                NetPacket p = NetManager.PoolGetPacket(newMtu);
                p.Property = PacketProperty.MtuCheck;
                FastBitConverter.GetBytes(p.RawData, 1, newMtu); //place into start
                FastBitConverter.GetBytes(p.RawData, p.Size - 4, newMtu); //and end of packet

                //Must check result for MTU fix
                if (NetManager.SendRawAndRecycle(p, this) <= 0)
                    _finishMtu = true;
            }
        }

        internal ConnectRequestResult ProcessConnectRequest(NetConnectRequestPacket connRequest)
        {
            //current or new request
            switch (ConnectionState)
            {
                //P2P case
                case ConnectionState.Outgoing:
                    //fast check
                    if (connRequest.ConnectionTime < ConnectTime)
                    {
                        return ConnectRequestResult.P2PLose;
                    }
                    //slow rare case check
                    if (connRequest.ConnectionTime == ConnectTime)
                    {
                        byte[] localBytes = connRequest.TargetAddress;
                        for (int i = _cachedSocketAddr.Size - 1; i >= 0; i--)
                        {
                            byte rb = _cachedSocketAddr[i];
                            if (rb == localBytes[i])
                                continue;
                            if (rb < localBytes[i])
                                return ConnectRequestResult.P2PLose;
                        }
                    }
                    break;

                case ConnectionState.Connected:
                    //Old connect request
                    if (connRequest.ConnectionTime == ConnectTime)
                    {
                        //just reply accept
                        NetManager.SendRaw(_connectAcceptPacket, this);
                    }
                    //New connect request
                    else if (connRequest.ConnectionTime > ConnectTime)
                    {
                        return ConnectRequestResult.Reconnection;
                    }
                    break;

                case ConnectionState.Disconnected:
                case ConnectionState.ShutdownRequested:
                    if (connRequest.ConnectionTime >= ConnectTime)
                        return ConnectRequestResult.NewConnection;
                    break;
            }
            return ConnectRequestResult.None;
        }

        //Process incoming packet
        internal void ProcessPacket(NetPacket packet)
        {
            //not initialized
            if (ConnectionState == ConnectionState.Outgoing || ConnectionState == ConnectionState.Disconnected)
            {
                NetManager.PoolRecycle(packet);
                return;
            }
            if (packet.Property == PacketProperty.ShutdownOk)
            {
                if (ConnectionState == ConnectionState.ShutdownRequested)
                    ConnectionState = ConnectionState.Disconnected;
                NetManager.PoolRecycle(packet);
                return;
            }
            if (packet.ConnectionNumber != _connectNum)
            {
                NetDebug.Write(NetLogLevel.Trace, "[RR]Old packet");
                NetManager.PoolRecycle(packet);
                return;
            }
            Interlocked.Exchange(ref _timeSinceLastPacket, 0);

            NetDebug.Write($"[RR]PacketProperty: {packet.Property}");
            switch (packet.Property)
            {
                case PacketProperty.Merged:
                    int pos = NetConstants.HeaderSize;
                    while (pos < packet.Size)
                    {
                        ushort size = BitConverter.ToUInt16(packet.RawData, pos);
                        if (size == 0)
                        {
                            break;
                        }
                        pos += 2;
                        if (packet.RawData.Length - pos < size)
                            break;

                        NetPacket mergedPacket = NetManager.PoolGetPacket(size);
                        Buffer.BlockCopy(packet.RawData, pos, mergedPacket.RawData, 0, size);
                        mergedPacket.Size = size;

                        if (!mergedPacket.Verify())
                            break;

                        pos += size;
                        ProcessPacket(mergedPacket);
                    }
                    NetManager.PoolRecycle(packet);
                    break;
                //If we get ping, send pong
                case PacketProperty.Ping:
                    if (NetUtils.RelativeSequenceNumber(packet.Sequence, _pongPacket.Sequence) > 0)
                    {
                        NetDebug.Write("[PP]Ping receive, send pong");
                        FastBitConverter.GetBytes(_pongPacket.RawData, 3, DateTime.UtcNow.Ticks);
                        _pongPacket.Sequence = packet.Sequence;
                        NetManager.SendRaw(_pongPacket, this);
                    }
                    NetManager.PoolRecycle(packet);
                    break;

                //If we get pong, calculate ping time and rtt
                case PacketProperty.Pong:
                    if (packet.Sequence == _pingPacket.Sequence)
                    {
                        _pingTimer.Stop();
                        int elapsedMs = (int)_pingTimer.ElapsedMilliseconds;
                        RemoteTimeDelta = BitConverter.ToInt64(packet.RawData, 3) + elapsedMs * TimeSpan.TicksPerMillisecond / 2 - DateTime.UtcNow.Ticks;
                        UpdateRoundTripTime(elapsedMs);
                        NetManager.ConnectionLatencyUpdated(this, elapsedMs / 2);
                        NetDebug.Write($"[PP]Ping: {packet.Sequence} - {elapsedMs} - {RemoteTimeDelta}");
                    }
                    NetManager.PoolRecycle(packet);
                    break;

                case PacketProperty.Ack:
                case PacketProperty.Channeled:
                    if (packet.ChannelId >= _channels.Length)
                    {
                        NetManager.PoolRecycle(packet);
                        break;
                    }
                    BaseChannel channel = _channels[packet.ChannelId] ?? (packet.Property == PacketProperty.Ack ? null : CreateChannel(packet.ChannelId));
                    if (channel != null)
                    {
                        if (!channel.ProcessPacket(packet))
                            NetManager.PoolRecycle(packet);
                    }
                    break;

                //Simple packet without acks
                case PacketProperty.Unreliable:
                    NetManager.CreateReceiveEvent(packet, DeliveryMethod.Unreliable, 0, NetConstants.HeaderSize, this);
                    return;

                case PacketProperty.MtuCheck:
                case PacketProperty.MtuOk:
                    ProcessMtuPacket(packet);
                    break;

                default:
                    NetDebug.WriteError("Error! Unexpected packet type: " + packet.Property);
                    break;
            }
        }

        private void SendMerged()
        {
            if (_mergeCount == 0)
                return;
            int bytesSent;
            if (_mergeCount > 1)
            {
                NetDebug.Write("[P]Send merged: " + _mergePos + ", count: " + _mergeCount);
                bytesSent = NetManager.SendRaw(_mergeData.RawData, 0, NetConstants.HeaderSize + _mergePos, this);
            }
            else
            {
                //Send without length information and merging
                bytesSent = NetManager.SendRaw(_mergeData.RawData, NetConstants.HeaderSize + 2, _mergePos - 2, this);
            }

            if (NetManager.EnableStatistics)
            {
                Statistics.IncrementPacketsSent();
                Statistics.AddBytesSent(bytesSent);
            }

            _mergePos = 0;
            _mergeCount = 0;
        }

        internal void SendUserData(NetPacket packet)
        {
            packet.ConnectionNumber = _connectNum;
            int mergedPacketSize = NetConstants.HeaderSize + packet.Size + 2;
            const int sizeTreshold = 20;
            if (mergedPacketSize + sizeTreshold >= Mtu)
            {
                NetDebug.Write(NetLogLevel.Trace, "[P]SendingPacket: " + packet.Property);
                int bytesSent = NetManager.SendRaw(packet, this);

                if (NetManager.EnableStatistics)
                {
                    Statistics.IncrementPacketsSent();
                    Statistics.AddBytesSent(bytesSent);
                }

                return;
            }
            if (_mergePos + mergedPacketSize > Mtu)
                SendMerged();

            FastBitConverter.GetBytes(_mergeData.RawData, _mergePos + NetConstants.HeaderSize, (ushort)packet.Size);
            Buffer.BlockCopy(packet.RawData, 0, _mergeData.RawData, _mergePos + NetConstants.HeaderSize + 2, packet.Size);
            _mergePos += packet.Size + 2;
            _mergeCount++;
            //DebugWriteForce("Merged: " + _mergePos + "/" + (_mtu - 2) + ", count: " + _mergeCount);
        }

        internal void Update(int deltaTime)
        {
            Interlocked.Add(ref _timeSinceLastPacket, deltaTime);
            switch (ConnectionState)
            {
                case ConnectionState.Connected:
                    if (_timeSinceLastPacket > NetManager.DisconnectTimeout)
                    {
                        NetDebug.Write($"[UPDATE] Disconnect by timeout: {_timeSinceLastPacket} > {NetManager.DisconnectTimeout}");
                        NetManager.DisconnectPeerForce(this, DisconnectReason.Timeout, 0, null);
                        return;
                    }
                    break;

                case ConnectionState.ShutdownRequested:
                    if (_timeSinceLastPacket > NetManager.DisconnectTimeout)
                    {
                        ConnectionState = ConnectionState.Disconnected;
                    }
                    else
                    {
                        _shutdownTimer += deltaTime;
                        if (_shutdownTimer >= ShutdownDelay)
                        {
                            _shutdownTimer = 0;
                            NetManager.SendRaw(_shutdownPacket, this);
                        }
                    }
                    return;

                case ConnectionState.Outgoing:
                    _connectTimer += deltaTime;
                    if (_connectTimer > NetManager.ReconnectDelay)
                    {
                        _connectTimer = 0;
                        _connectAttempts++;
                        if (_connectAttempts > NetManager.MaxConnectAttempts)
                        {
                            NetManager.DisconnectPeerForce(this, DisconnectReason.ConnectionFailed, 0, null);
                            return;
                        }

                        //else send connect again
                        NetManager.SendRaw(_connectRequestPacket, this);
                    }
                    return;

                case ConnectionState.Disconnected:
                    return;
            }

            //Send ping
            _pingSendTimer += deltaTime;
            if (_pingSendTimer >= NetManager.PingInterval)
            {
                NetDebug.Write("[PP] Send ping...");
                //reset timer
                _pingSendTimer = 0;
                //send ping
                _pingPacket.Sequence++;
                //ping timeout
                if (_pingTimer.IsRunning)
                    UpdateRoundTripTime((int)_pingTimer.ElapsedMilliseconds);
                _pingTimer.Restart();
                NetManager.SendRaw(_pingPacket, this);
            }

            //RTT - round trip time
            _rttResetTimer += deltaTime;
            if (_rttResetTimer >= NetManager.PingInterval * 3)
            {
                _rttResetTimer = 0;
                _rtt = RoundTripTime;
                _rttCount = 1;
            }

            UpdateMtuLogic(deltaTime);

            //Pending send
            int count = _channelSendQueue.Count;
            while (count-- > 0)
            {
                if (!_channelSendQueue.TryDequeue(out BaseChannel channel))
                    break;
                if (channel.SendAndCheckQueue())
                {
                    // still has something to send, re-add it to the send queue
                    _channelSendQueue.Enqueue(channel);
                }
            }

            lock (_unreliableChannel)
            {
                int unreliableCount = _unreliableChannel.Count;
                for (int i = 0; i < unreliableCount; i++)
                {
                    NetPacket packet = _unreliableChannel.Dequeue();
                    SendUserData(packet);
                    NetManager.PoolRecycle(packet);
                }
            }

            SendMerged();
        }

        //For reliable channel
        internal void RecycleAndDeliver(NetPacket packet)
        {
            if (packet.UserData != null)
            {
                NetManager.MessageDelivered(this, packet.UserData);
                packet.UserData = null;
            }
            NetManager.PoolRecycle(packet);
        }
    }
}
