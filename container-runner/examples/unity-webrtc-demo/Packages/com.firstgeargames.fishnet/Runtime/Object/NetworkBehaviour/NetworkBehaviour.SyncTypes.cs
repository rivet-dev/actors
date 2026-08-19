#if UNITY_EDITOR || DEVELOPMENT_BUILD
#define DEVELOPMENT
#endif
using FishNet.CodeGenerating;
using FishNet.Connection;
using FishNet.Managing;
using FishNet.Managing.Transporting;
using FishNet.Object.Synchronizing;
using FishNet.Object.Synchronizing.Internal;
using FishNet.Serializing;
using FishNet.Transporting;
using GameKit.Dependencies.Utilities;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using FishNet.Serializing.Helping;
using FishNet.Utility.Extension;
using UnityEngine;

namespace FishNet.Object
{
    public abstract partial class NetworkBehaviour : MonoBehaviour
    {
        #region Types.
        /// <summary>
        /// Used to generate data sent from synctypes.
        /// </summary>
        private struct SyncTypeWriter
        {
            /// <summary>
            /// Writers for each channel.
            /// </summary>
            public List<PooledWriter> Writers;

            /// <summary>
            /// Resets Writers.
            /// </summary>
            public void Reset()
            {
                if (Writers == null)
                    return;

                for (int i = 0; i < Writers.Count; i++)
                    Writers[i].Clear();
            }

            public void Initialize()
            {
                Writers = CollectionCaches<PooledWriter>.RetrieveList();
                for (int i = 0; i < TransportManager.CHANNEL_COUNT; i++)
                    Writers.Add(WriterPool.Retrieve());
            }
        }
        #endregion

        #region Private.
        /// <summary>
        /// Writers for syncTypes. A writer will exist for every ReadPermission type.
        /// </summary>
        private static Dictionary<ReadPermission, SyncTypeWriter> _syncTypeWriters = new();
        /// <summary>
        /// SyncTypes within this NetworkBehaviour.
        /// </summary>
        private Dictionary<uint, SyncBase> _syncTypes 
        {
            get
            {
                if (_syncTypesCache == null)
                    _syncTypesCache = CollectionCaches<uint, SyncBase>.RetrieveDictionary();

                return _syncTypesCache;
            }
        }
        private Dictionary<uint, SyncBase> _syncTypesCache;
        /// <summary>
        /// True if at least one syncType is dirty.
        /// </summary>
        internal bool SyncTypeDirty;
        /// <summary>
        /// All ReadPermission values.
        /// This is used to build SyncTypeWriters on initialization.
        /// </summary>
        private static List<ReadPermission> _readPermissions;
        #endregion

        #region Consts.
        /// <summary>
        /// Bytes to reserve for writing SyncType headers.
        /// </summary>
        /// <returns></returns>
        internal const byte SYNCTYPE_RESERVE_BYTES = 4;
        /// <summary>
        /// Bytes to reserve for writing payload headers.
        /// </summary>
        /// <returns></returns>
        internal const byte PAYLOAD_RESERVE_BYTES = 4;
        #endregion

        /// <summary>
        /// Registers a SyncType.
        /// </summary>
        /// <param name = "sb"></param>
        /// <param name = "index"></param>
        internal void RegisterSyncType(SyncBase sb, uint index)
        {
            if (!_syncTypes.TryAdd(index, sb))
                NetworkManager.LogError($"SyncType key {index} has already been added for {GetType().FullName} on {gameObject.name}");
        }

        /// <summary>
        /// Sets a SyncType as dirty.
        /// </summary>
        /// <returns>True if able to dirty SyncType.</returns>
        internal bool DirtySyncType()
        {
            if (!IsServerStarted)
                return false;
            /* No reason to dirty if there are no observers.
             * This can happen even if a client is going to see
             * this object because the server side initializes
             * before observers are built. Clients which become observers
             * will get the latest values in the spawn message, which is separate
             * from writing dirty syncTypes. */
            if (_networkObjectCache.Observers.Count == 0 && !_networkObjectCache.PredictedSpawner.IsValid)
                return false;
            if (!SyncTypeDirty)
                _networkObjectCache.NetworkManager.ServerManager.Objects.SetDirtySyncType(this);

            SyncTypeDirty = true;

            return true;
        }

        /// <summary>
        /// Initializes SyncTypes. This will only call once even as host.
        /// </summary>
        private void SyncTypes_Preinitialize(bool asServer)
        {
            if (_networkObjectCache.DoubleLogic(asServer))
                return;

            // This only runs once since SyncTypeWriters are static.
            if (_syncTypeWriters.Count == 0)
            {
                List<ReadPermission> readPermissions = new();
                System.Array arr = System.Enum.GetValues(typeof(ReadPermission));
                foreach (ReadPermission rp in arr)
                    readPermissions.Add(rp);

                foreach (ReadPermission rp in readPermissions)
                {
                    SyncTypeWriter syncTypeWriter = new();
                    syncTypeWriter.Initialize();
                    _syncTypeWriters[rp] = syncTypeWriter;
                }
            }

            /* Initialize synctypes every spawn because there could be
             * callbacks which occur that the user or even we may implement
             * during the initialization. */
            foreach (SyncBase sb in _syncTypes.Values)
                sb.PreInitialize(_networkObjectCache.NetworkManager, asServer);
        }

        /// <summary>
        /// Reads a SyncType.
        /// </summary>
        internal void ReadSyncType(int readerPositionAfterDebug, PooledReader reader, int writtenLength, bool asServer = false)
        {
            int endPosition = reader.Position + writtenLength;
            while (reader.Position < endPosition)
            {
                byte syncTypeId = reader.ReadUInt8Unpacked();
                if (_syncTypes.TryGetValueIL2CPP(syncTypeId, out SyncBase sb))
                    sb.Read(reader, asServer);
                else
                    NetworkManager.LogError($"SyncType not found for index {syncTypeId} on {transform.name}, component {GetType().FullName}. The remainder of the packet will become corrupt likely resulting in unforeseen issues for this tick, such as data missing or objects not spawning.");
            }

            if (reader.Position > endPosition)
            {
                NetworkManager.LogError($"Remaining bytes in SyncType reader are less than expected. Something did not serialize or deserialize properly which will likely result in a SyncType being incorrect.");
                // Fix position.
                reader.Position = endPosition;
            }

#if DEVELOPMENT && !UNITY_SERVER
            if (_networkTrafficStatistics != null)
                _networkTrafficStatistics.AddOutboundPacketIdData(PacketId.SyncType, _typeName, reader.Position - readerPositionAfterDebug, gameObject, asServer: false);
#endif
        }

        /// <summary>
        /// Writes only dirty SyncTypes.
        /// </summary>
        /// <returns>True if there are no pending dirty sync types.</returns>
        internal bool WriteDirtySyncTypes(SyncTypeWriteFlag flags)
        {
            // /* IsSpawned Can occur when a synctype is queued after
            //  * the object is marked for destruction. This should not
            //  * happen under most conditions since synctypes will be
            //  * pushed through when despawn is called.
            //  *
            //  * No observers can occur when the server changes a syncType
            //  * value but gained no observers in the same tick. We still
            //  * want to mark a syncType as dirty in this situation because
            //  * it needs to write in a despawn message in the scenario the object
            //  * is spawned (no observers), synctype changed, then despawned immediately
            //  * after.
            //  */
            // if (!IsSpawned || _networkObjectCache.Observers.Count == 0)
            // {
            //     ResetState_SyncTypes(asServer: true);
            //     return true;
            // }

            /* IsSpawned Can occur when a synctype is queued after
             * the object is marked for destruction. This should not
             * happen under most conditions since synctypes will be
             * pushed through when despawn is called. */
            if (!IsSpawned)
            {
                ResetState_SyncTypes(asServer: true);
                return true;
            }

            /* Additional checks need to appear below the reset check
             * above. Resets should place priority as this method was called
             * when it should not have been, such as during a despawn. */

            // None dirty or no synctypes.
            if (!SyncTypeDirty || _syncTypes.Count == 0)
                return true;

            // Number of syncTypes which are/were dirty.
            int dirtyCount = 0;
            // Number of syncTypes which were written.
            int writtenCount = 0;

            // Flags as boolean.
            bool ignoreInterval = flags.FastContains(SyncTypeWriteFlag.IgnoreInterval);
            bool forceReliable = flags.FastContains(SyncTypeWriteFlag.ForceReliable);

            uint tick = _networkObjectCache.NetworkManager.TimeManager.Tick;
            bool ownerIsActive = _networkObjectCache.Owner.IsActive;

            // Reset syncTypeWriters.
            foreach (SyncTypeWriter stw in _syncTypeWriters.Values)
                stw.Reset();

            HashSet<ReadPermission> writtenReadPermissions = CollectionCaches<ReadPermission>.RetrieveHashSet();

            foreach (SyncBase sb in _syncTypes.Values)
            {
                // This entry is not dirty.
                if (!sb.IsDirty)
                    continue;

                /* Mark that at least one is still dirty.
                 * This does not mean that anything was written
                 * as there are still blocks to bypass. */
                dirtyCount++;

                // Interval not yet met.
                if (!ignoreInterval && !sb.IsNextSyncTimeMet(tick))
                    continue;

                // Unset that SyncType is dirty as it will be written now.
                sb.ResetDirty();

                /* SyncType is for owner only but the owner is not valid, therefor
                 * nothing can be written. It's possible for a SyncType to be dirty
                 * and owner only, with no owner, if the owner dropped after the syncType
                 * was dirtied. */
                ReadPermission rp = sb.Settings.ReadPermission;
                // If ReadPermission is owner but no owner skip this syncType write.
                if (!ownerIsActive && rp == ReadPermission.OwnerOnly)
                    continue;

                writtenCount++;

                if (forceReliable)
                    sb.SetCurrentChannel(Channel.Reliable);

                // Get channel
                byte channel = (byte)sb.Channel;

                /* Writer can be obtained quickly by using the readPermission byte value.
                 * Byte values are in order starting at 0. */


                // Find writer to use. Should never fail.
                if (!_syncTypeWriters.TryGetValueIL2CPP(rp, out SyncTypeWriter stw))
                    continue;

                /* Channel for syncType is beyond available channels in transport.
                 * Use default reliable. */
                if (channel >= TransportManager.CHANNEL_COUNT)
                    channel = (byte)Channel.Reliable;

                writtenReadPermissions.Add(rp);

                sb.WriteDelta(stw.Writers[channel]);
            }

            // If no dirty were found.
            if (dirtyCount == 0)
            {
                SyncTypeDirty = false;
                CollectionCaches<ReadPermission>.Store(writtenReadPermissions);
                return true;
            }

            // Nothing was written, but some are still dirty.
            if (writtenReadPermissions.Count == 0)
            {
                CollectionCaches<ReadPermission>.Store(writtenReadPermissions);
                return false;
            }

            /* If here something was written. */

            PooledWriter fullWriter = WriterPool.Retrieve();
            TransportManager tm = _networkObjectCache.NetworkManager.TransportManager;

#if DEVELOPMENT && !UNITY_SERVER
            int totalBytesWritten = 0;
#endif

            foreach (ReadPermission rp in writtenReadPermissions)
            {
                // Find writer to use. Should never fail.
                if (!_syncTypeWriters.TryGetValueIL2CPP(rp, out SyncTypeWriter stw))
                    continue;

                for (int i = 0; i < stw.Writers.Count; i++)
                {
                    PooledWriter writer = stw.Writers[i];
                    // None written for this channel.
                    if (writer.Length == 0)
                        continue;

                    CompleteSyncTypePacket(fullWriter, writer);
                    writer.Clear();

                    // Should not be the case but check for safety.
                    if (fullWriter.Length == 0)
                        continue;

                    byte channel = (byte)i;

                    switch (rp)
                    {
                        // Send to everyone or excludeOwner.
                        case ReadPermission.Observers:
                            tm.SendToClients(channel, fullWriter.GetArraySegment(), _networkObjectCache.Observers);
                            break;
                        // Everyone but owner.
                        case ReadPermission.ExcludeOwner:
                            _networkConnectionCache.Clear();
                            if (ownerIsActive)
                                _networkConnectionCache.Add(_networkObjectCache.Owner);
                            tm.SendToClients(channel, fullWriter.GetArraySegment(), _networkObjectCache.Observers, _networkConnectionCache);
                            break;
                        // Owner only. Owner will always be valid if here.
                        case ReadPermission.OwnerOnly:
                            tm.SendToClient(channel, fullWriter.GetArraySegment(), _networkObjectCache.Owner);
                            break;
                    }

#if DEVELOPMENT && !UNITY_SERVER
                    totalBytesWritten += fullWriter.Length;
#endif

                    fullWriter.Clear();
                }
            }

#if DEVELOPMENT && !UNITY_SERVER
            if (_networkTrafficStatistics != null)
                _networkTrafficStatistics.AddOutboundPacketIdData(PacketId.SyncType, _typeName, totalBytesWritten, gameObject, asServer: true);
#endif
            fullWriter.Store();
            CollectionCaches<ReadPermission>.Store(writtenReadPermissions);

            // Return if all dirty were written.
            bool allDirtyWritten = dirtyCount == writtenCount;
            if (allDirtyWritten)
                SyncTypeDirty = false;

            return allDirtyWritten;
        }

        /// <summary>
        /// Writes all SyncTypes for a connection if readPermissions match.
        /// </summary>
        internal void WriteSyncTypesForConnection(NetworkConnection conn, ReadPermission readPermissions)
        {
            // There are no syncTypes.
            if (_syncTypes.Count == 0)
                return;

            // It will always exist but we need to out anyway.
            if (!_syncTypeWriters.TryGetValueIL2CPP(readPermissions, out SyncTypeWriter stw))
                return;

            // Reset syncTypeWriters.
            stw.Reset();

            PooledWriter fullWriter = WriterPool.Retrieve();

            foreach (SyncBase sb in _syncTypes.Values)
            {
                if (sb.Settings.ReadPermission != readPermissions)
                    continue;

                PooledWriter writer = stw.Writers[(byte)sb.Settings.Channel];
                sb.WriteFull(writer);
            }

#if DEVELOPMENT && !UNITY_SERVER
            int totalBytesWritten = 0;
#endif

            for (int i = 0; i < stw.Writers.Count; i++)
            {
                PooledWriter writer = stw.Writers[i];
                CompleteSyncTypePacket(fullWriter, writer);
                writer.Clear();

                byte channel = (byte)Channel.Reliable;
                _networkObjectCache.NetworkManager.TransportManager.SendToClient(channel, fullWriter.GetArraySegment(), conn);
            }

#if DEVELOPMENT && !UNITY_SERVER
            if (_networkTrafficStatistics != null)
                _networkTrafficStatistics.AddOutboundPacketIdData(PacketId.SyncType, _typeName, totalBytesWritten, gameObject, asServer: true);
#endif

            fullWriter.Store();
        }

        /// <summary>
        /// Completes the writing of a SyncType by writing the header and serialized values.
        /// </summary>
        private void CompleteSyncTypePacket(PooledWriter fullWriter, PooledWriter syncTypeWriter)
        {
            // None written for this writer.
            if (syncTypeWriter.Length == 0)
                return;

            fullWriter.Clear();
            fullWriter.WritePacketIdUnpacked(PacketId.SyncType);
            fullWriter.WriteNetworkBehaviour(this);

            ReservedLengthWriter reservedWriter = ReservedWritersExtensions.Retrieve();
            reservedWriter.Initialize(fullWriter, SYNCTYPE_RESERVE_BYTES);

            fullWriter.WriteArraySegment(syncTypeWriter.GetArraySegment());

            reservedWriter.WriteLength();
            reservedWriter.Store();
        }

        /// <summary>
        /// Writes syncTypes for a spawn message.
        /// </summary>
        /// <param name = "conn">Connection SyncTypes are being written for.</param>
        internal void WriteSyncTypesForSpawn(PooledWriter writer, NetworkConnection conn)
        {
            // There are no syncTypes.
            if (_syncTypes.Count == 0)
                return;

            // True if connection passed in is the owner of this object.
            bool connIsOwner = conn == _networkObjectCache.Owner;

            // Reserved bytes for componentIndex and amount written.
            const byte reservedBytes = 2;
            writer.Skip(reservedBytes);
            int positionAfterReserve = writer.Position;

            byte written = 0;

            foreach (SyncBase sb in _syncTypes.Values)
            {
                ReadPermission rp = sb.Settings.ReadPermission;
                bool canWrite = rp == ReadPermission.Observers || (rp == ReadPermission.ExcludeOwner && !connIsOwner) || (rp == ReadPermission.OwnerOnly && connIsOwner);

                if (!canWrite)
                    continue;

                int startWriterPosition = writer.Position;
                sb.WriteFull(writer);
                if (writer.Position != startWriterPosition)
                    written++;
            }

            // If any where written.
            if (positionAfterReserve != writer.Position)
            {
                int insertPosition = positionAfterReserve - reservedBytes;
                writer.InsertUInt8Unpacked(ComponentIndex, insertPosition++);
                writer.InsertUInt8Unpacked(written, insertPosition);
            }
            else
            {
                writer.Remove(reservedBytes);
            }
        }

        /// <summary>
        /// Reads a SyncType for spawn.
        /// </summary>
        internal void ReadSyncTypesForSpawn(PooledReader reader)
        {
            byte written = reader.ReadUInt8Unpacked();
            for (int i = 0; i < written; i++)
            {
                byte syncTypeId = reader.ReadUInt8Unpacked();

                if (_syncTypes.TryGetValueIL2CPP(syncTypeId, out SyncBase sb))
                    sb.Read(reader, asServer: false);
                else
                    NetworkManager.LogWarning($"SyncType not found for index {syncTypeId} on {transform.name}, component {GetType().FullName}. Remainder of packet may become corrupt.");
            }
        }

        /// <summary>
        /// Resets all SyncTypes for this NetworkBehaviour for server or client.
        /// </summary>
        internal void ResetState_SyncTypes(bool asServer)
        {
            if (_syncTypes != null)
            {
                foreach (SyncBase item in _syncTypes.Values)
                    item.ResetState(asServer);
            }

            if (_syncTypeWriters != null)
            {
                foreach (SyncTypeWriter syncTypeWriter in _syncTypeWriters.Values)
                    syncTypeWriter.Reset();
            }

            if (asServer)
                SyncTypeDirty = false;
        }

        private void SyncTypes_OnDestroy()
        {
            CollectionCaches<uint, SyncBase>.StoreAndDefault(ref _syncTypesCache);
        }
    }
}