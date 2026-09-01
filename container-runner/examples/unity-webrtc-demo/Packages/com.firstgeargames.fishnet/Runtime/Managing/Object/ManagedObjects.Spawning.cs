#if UNITY_EDITOR || DEVELOPMENT_BUILD
#define DEVELOPMENT
#endif
using System;
using FishNet.Connection;
using FishNet.Managing.Logging;
using FishNet.Managing.Server;
using FishNet.Object;
using FishNet.Serializing;
using FishNet.Transporting;
using GameKit.Dependencies.Utilities;
using System.Collections.Generic;
using FishNet.Serializing.Helping;
using UnityEngine;

namespace FishNet.Managing.Object
{
    public abstract partial class ManagedObjects
    {
        #region Consts.
        /// <summary>
        /// Number of bytes to reserve for a predicted spawn length.
        /// </summary>
        internal const byte PREDICTED_SPAWN_BYTES = 2;
        #endregion

        /// <summary>
        /// Reads and outputs a transforms values.
        /// </summary>
        protected void ReadTransformProperties(Reader reader, out Vector3? localPosition, out Quaternion? localRotation, out Vector3? localScale)
        {
            // Read changed.
            TransformPropertiesFlag tpf = (TransformPropertiesFlag)reader.ReadUInt8Unpacked();
            // Position.
            if (tpf.FastContains(TransformPropertiesFlag.Position))
                localPosition = reader.ReadVector3();
            else
                localPosition = null;
            // Rotation.
            if (tpf.FastContains(TransformPropertiesFlag.Rotation))
                localRotation = reader.ReadQuaternion(NetworkManager.ServerManager.SpawnPacking.Rotation);
            else
                localRotation = null;
            // Scale.
            if (tpf.FastContains(TransformPropertiesFlag.Scale))
                localScale = reader.ReadVector3();
            else
                localScale = null;
        }

        /// <summary>
        /// Writes a spawn to a client or server.
        /// If connection is not null the spawn is sent ot a client, otherwise it will be considered a predicted spawn.
        /// </summary>
        /// <returns>True if spawn was written.</returns>
        internal bool WriteSpawn(NetworkObject nob, PooledWriter writer, NetworkConnection connection)
        {
            writer.WritePacketIdUnpacked(PacketId.ObjectSpawn);

            ReservedLengthWriter asClientReservedWriter = ReservedWritersExtensions.Retrieve();
            bool predictedSpawn = connection == null;

            if (predictedSpawn)
                asClientReservedWriter.Initialize(writer, PREDICTED_SPAWN_BYTES);

            bool sceneObject = nob.IsSceneObject;
            // Write type of spawn.
            SpawnType st = SpawnType.Unset;
            if (sceneObject)
                st |= SpawnType.Scene;
            else
                st |= nob.IsGlobal ? SpawnType.InstantiatedGlobal : SpawnType.Instantiated;

            if (connection == nob.PredictedSpawner)
                st |= SpawnType.IsPredictedSpawner;

            // Call before writing SpawnType so nested can be appended to it if needed.
            PooledWriter nestedWriter = WriteNestedSpawn(nob, ref st);

            writer.WriteUInt8Unpacked((byte)st);
            // Write parent here if writer for parent is valid.
            if (nestedWriter != null)
            {
                writer.WriteArraySegment(nestedWriter.GetArraySegment());
                WriterPool.Store(nestedWriter);
            }

            writer.WriteSpawnedNetworkObject(nob);
            writer.WriteNetworkConnection(nob.Owner);

            // Properties on the transform which diff from serialized value.
            WriteChangedTransformProperties(nob, sceneObject, writer);

            /* Writing a scene object. */
            if (sceneObject)
            {
                writer.WriteUInt64Unpacked(nob.SceneId);
#if DEVELOPMENT
                CheckWriteSceneObjectDetails(nob, writer);
#endif
            }
            /* Writing a spawned object. */
            else
            {
                writer.WriteNetworkObjectId(nob.PrefabId);
            }

            NetworkConnection payloadSender = predictedSpawn ? NetworkManager.EmptyConnection : connection;
            WritePayload(payloadSender, nob, writer);

            /* RPCLinks and SyncTypes are ONLY written by the server.
             * Although not necessary, both sides will write the length
             * to keep the reading of spawns consistent. */
            WriteRpcLinks(nob, writer);
            WriteSyncTypesForSpawn(nob, writer, connection);

            bool canWrite;
            // Need to validate predicted spawn length.
            if (predictedSpawn)
            {
                int maxContentLength;
                if (PREDICTED_SPAWN_BYTES == 2)
                {
                    maxContentLength = ushort.MaxValue;
                }
                else
#pragma warning disable CS0162 // Unreachable code detected
                {
                    NetworkManager.LogError($"Unhandled spawn bytes value of {PREDICTED_SPAWN_BYTES}.");
                    maxContentLength = 0;
                }
#pragma warning restore CS0162 // Unreachable code detected

                // Too much content; this really should absolutely never happen.
                canWrite = asClientReservedWriter.Length <= maxContentLength;
                if (!canWrite)
                    NetworkManager.LogError($"A single predicted spawns may not exceed {maxContentLength} bytes in length. Written length is {asClientReservedWriter.Length}. Predicted spawn for {nob.name} will be despawned immediately.");
                // Not too large.
                else
                    asClientReservedWriter.WriteLength();
            }

            // Not predicted, server can always write.
            else
            {
                canWrite = true;
            }

            asClientReservedWriter.Store();
            return canWrite;
        }

        /// <summary>
        /// Writes RPCLinks for a NetworkObject.
        /// </summary>
        protected void WriteRpcLinks(NetworkObject nob, PooledWriter writer)
        {
            ReservedLengthWriter rw = ReservedWritersExtensions.Retrieve();

            rw.Initialize(writer, NetworkBehaviour.RPCLINK_RESERVED_BYTES);

            if (NetworkManager.IsServerStarted)
            {
                foreach (NetworkBehaviour nb in nob.NetworkBehaviours)
                    nb.WriteRpcLinks(writer);
            }

            rw.WriteLength();

            rw.Store();
        }

        /// <summary>
        /// Reads RpcLinks from a spawn into an arraySegment.
        /// </summary>
        protected ArraySegment<byte> ReadRpcLinks(PooledReader reader)
        {
            uint segmentSize = ReservedLengthWriter.ReadLength(reader, NetworkBehaviour.RPCLINK_RESERVED_BYTES);
            return reader.ReadArraySegment((int)segmentSize);
        }

        /// <summary>
        /// Writes SyncTypes for a NetworkObject.
        /// </summary>
        protected void WriteSyncTypesForSpawn(NetworkObject nob, PooledWriter writer, NetworkConnection connection)
        {
            ReservedLengthWriter rw = ReservedWritersExtensions.Retrieve();

            // SyncTypes.
            rw.Initialize(writer, NetworkBehaviour.SYNCTYPE_RESERVE_BYTES);

            if (NetworkManager.IsServerStarted)
            {
                foreach (NetworkBehaviour nb in nob.NetworkBehaviours)
                    nb.WriteSyncTypesForSpawn(writer, connection);
            }

            rw.WriteLength();
            rw.Store();
        }

        /// <summary>
        /// Reads SyncTypes from a spawn into an arraySegment.
        /// </summary>
        protected ArraySegment<byte> ReadSyncTypesForSpawn(PooledReader reader)
        {
            uint segmentSize = ReservedLengthWriter.ReadLength(reader, NetworkBehaviour.SYNCTYPE_RESERVE_BYTES);
            return reader.ReadArraySegment((int)segmentSize);
        }

        /// <summary>
        /// Writers a nested spawn and returns writer used.
        /// If nested was not written null is returned.
        /// </summary>
        internal PooledWriter WriteNestedSpawn(NetworkObject nob, ref SpawnType st)
        {
            // Check to write parent behaviour or nob.
            NetworkBehaviour parentNb;
            Transform t = nob.transform.parent;
            if (t != null)
            {
                parentNb = nob.CurrentParentNetworkBehaviour;
                /* Check for a NetworkObject if there is no NetworkBehaviour.
                 * There is a small chance the parent object will only contain
                 * a NetworkObject. */
                if (parentNb == null)
                {
                    return null;
                }
                // No parent.
                else
                {
                    if (!parentNb.IsSpawned)
                    {
                        NetworkManager.LogWarning($"Parent {t.name} is not spawned. {nob.name} will not have it's parent sent in the spawn message.");
                        return null;
                    }
                    else
                    {
                        st |= SpawnType.Nested;
                        PooledWriter writer = WriterPool.Retrieve();
                        writer.WriteUInt8Unpacked(nob.ComponentIndex);
                        writer.WriteNetworkBehaviour(parentNb);
                        return writer;
                    }
                }
            }
            // CurrentNetworkBehaviour is not set.
            else
            {
                return null;
            }
        }

        /// <summary>
        /// If flags indicate there is a nested spawn the objectId and NetworkBehaviourId are output.
        /// Otherwise, output value sare set to null.
        /// </summary>
        internal void ReadNestedSpawnIds(PooledReader reader, SpawnType st, out byte? nobComponentIndex, out int? parentObjectId, out byte? parentComponentIndex, HashSet<int> readSpawningObjects = null)
        {
            if (st.FastContains(SpawnType.Nested))
            {
                nobComponentIndex = reader.ReadUInt8Unpacked();
                reader.ReadNetworkBehaviour(out int objectId, out byte componentIndex, readSpawningObjects);
                if (objectId != NetworkObject.UNSET_OBJECTID_VALUE)
                {
                    parentObjectId = objectId;
                    parentComponentIndex = componentIndex;
                    return;
                }
            }

            // Fall through, not nested.
            nobComponentIndex = null;
            parentObjectId = null;
            parentComponentIndex = null;
        }

        /// <summary>
        /// Finishes reading a scene object.
        /// </summary>
        protected void ReadSceneObjectId(PooledReader reader, out ulong sceneId)
        {
            sceneId = reader.ReadUInt64Unpacked();
        }

        /// <summary>
        /// Writes changed transform proeprties to writer.
        /// </summary>
        protected void WriteChangedTransformProperties(NetworkObject nob, bool sceneObject, Writer headerWriter)
        {
            /* Write changed transform properties. */
            TransformPropertiesFlag tpf;
            /* If a scene object or nested during initialization then
             * write changes compared to initialized values. */
            if (sceneObject || nob.InitializedParentNetworkBehaviour != null)
            {
                tpf = nob.GetTransformChanges(nob.SerializedTransformProperties);
            }
            else
            {
                // This should not be possible when spawning non-nested.
                if (nob.PrefabId == NetworkObject.UNSET_PREFABID_VALUE)
                {
                    NetworkManager.LogWarning($"NetworkObject {nob.ToString()} unexpectedly has an unset PrefabId while it's not nested. Please report this warning.");
                    tpf = TransformPropertiesFlag.Everything;
                }
                else
                {
                    PrefabObjects po = NetworkManager.GetPrefabObjects<PrefabObjects>(nob.SpawnableCollectionId, false);
                    tpf = nob.GetTransformChanges(po.GetObject(asServer: true, nob.PrefabId).gameObject);
                }
            }

            headerWriter.WriteUInt8Unpacked((byte)tpf);
            // If properties have changed.
            if (tpf != TransformPropertiesFlag.Unset)
            {
                // Write any changed properties.
                if (tpf.FastContains(TransformPropertiesFlag.Position))
                    headerWriter.WriteVector3(nob.transform.localPosition);
                if (tpf.FastContains(TransformPropertiesFlag.Rotation))
                    headerWriter.WriteQuaternion(nob.transform.localRotation, NetworkManager.ServerManager.SpawnPacking.Rotation);
                if (tpf.FastContains(TransformPropertiesFlag.Scale))
                    headerWriter.WriteVector3(nob.transform.localScale);
            }
        }

        /// <summary>
        /// Writes a despawn.
        /// </summary>
        protected void WriteDespawn(NetworkObject nob, DespawnType despawnType, Writer everyoneWriter)
        {
            everyoneWriter.WritePacketIdUnpacked(PacketId.ObjectDespawn);
            everyoneWriter.WriteNetworkObjectForDespawn(nob, despawnType);
        }

        /// <summary>
        /// Finds a scene NetworkObject and sets transform values.
        /// </summary>
        internal NetworkObject GetSceneNetworkObject(ulong sceneId, string sceneName, string objectName)
        {
            NetworkObject nob;
            SceneObjects_Internal.TryGetValueIL2CPP(sceneId, out nob);
            // If found in scene objects.
            if (nob == null)
            {
#if DEVELOPMENT
                string missingObjectDetails = sceneName == string.Empty ? "For more information on the missing object add DebugManager to your NetworkManager and enable WriteSceneObjectDetails" : $"Scene containing the object is '{sceneName}', object name is '{objectName}";
                NetworkManager.LogError($"SceneId of {sceneId} not found in SceneObjects. {missingObjectDetails}. This may occur if your scene differs between client and server, if client does not have the scene loaded, or if networked scene objects do not have a SceneCondition. See ObserverManager in the documentation for more on conditions.");
#else
                NetworkManager.LogError($"SceneId of {sceneId} not found in SceneObjects. This may occur if your scene differs between client and server, if client does not have the scene loaded, or if networked scene objects do not have a SceneCondition. See ObserverManager in the documentation for more on conditions.");
#endif
            }

            return nob;
        }

        /// <summary>
        /// Returns if a NetworkObject meets basic criteria for being predicted spawned.
        /// </summary>
        /// <param name = "reader">If not null reader will be cleared on error.</param>
        /// <returns></returns>
        protected bool CanPredictedSpawn(NetworkObject nob, NetworkConnection spawner, bool asServer, Reader reader = null)
        {
            // Does not allow predicted spawning.
            if (!nob.AllowPredictedSpawning)
            {
                if (asServer)
                    spawner.Kick(KickReason.ExploitAttempt, LoggingType.Common, $"Connection {spawner.ClientId} tried to spawn an object {nob.name} which does not support predicted spawning.");
                else
                    NetworkManager.LogError($"Object {nob.name} does not support predicted spawning. Add a PredictedSpawn component to the object and configure appropriately.");

                if (reader != null)
                    reader.Clear();
                return false;
            }

            // // Parenting is not yet supported.
            // if (nob.CurrentParentNetworkBehaviour != null)
            // {
            //     if (asServer)
            //         spawner.Kick(KickReason.ExploitAttempt, LoggingType.Common, $"Connection {spawner.ClientId} tried to spawn an object that is not root.");
            //     else
            //         NetworkManager.LogError($"Predicted spawning as a child is not supported.");
            //
            //     if (reader != null)
            //         reader.Clear();
            //     return false;
            // }

            // Nested nobs not yet supported.
            if (nob.InitializedNestedNetworkObjects.Count > 0)
            {
                if (asServer)
                    spawner.Kick(KickReason.ExploitAttempt, LoggingType.Common, $"Connection {spawner.ClientId} tried to spawn an object {nob.name} which has nested NetworkObjects.");
                else
                    NetworkManager.LogError($"Predicted spawning prefabs which contain nested NetworkObjects is not yet supported but will be in a later release.");

                if (reader != null)
                    reader.Clear();
                return false;
            }

            return true;
        }

        /// <summary>
        /// Returns if a NetworkObject meets basic criteria for being predicted despawned.
        /// </summary>
        /// <param name = "reader">If not null reader will be cleared on error.</param>
        /// <returns></returns>
        protected bool CanPredictedDespawn(NetworkObject nob, NetworkConnection despawner, bool asServer, Reader reader = null)
        {
            // Does not allow predicted spawning.
            if (!nob.AllowPredictedDespawning)
            {
                if (asServer)
                    despawner.Kick(KickReason.ExploitAttempt, LoggingType.Common, $"Connection {despawner.ClientId} tried to despawn an object {nob.name} which does not support predicted despawning.");
                else
                    NetworkManager.LogError($"Object {nob.name} does not support predicted despawning. Modify the PredictedSpawn component settings to allow predicted despawning.");

                reader?.Clear();
                return false;
            }

            ////Parenting is not yet supported.
            // if (nob.transform.parent != null)
            // {
            //    if (asServer)
            //        despawner.Kick(KickReason.ExploitAttempt, LoggingType.Common, $"Connection {despawner.ClientId} tried to despawn an object that is not root.");
            //    else
            //        NetworkManager.LogError($"Predicted despawning as a child is not supported.");

            //    reader?.Clear();
            //    return false;
            // }
            // Nested nobs not yet supported.
            if (nob.InitializedNestedNetworkObjects.Count > 0)
            {
                if (asServer)
                    despawner.Kick(KickReason.ExploitAttempt, LoggingType.Common, $"Connection {despawner.ClientId} tried to despawn an object {nob.name} which has nested NetworkObjects.");
                else
                    NetworkManager.LogError($"Predicted despawning prefabs which contain nested NetworkObjects is not yet supported but will be in a later release.");

                reader?.Clear();
                return false;
            }

            // Blocked by PredictedSpawn settings or user logic.
            if ((asServer && !nob.PredictedSpawn.OnTryDespawnServer(despawner)) || (!asServer && !nob.PredictedSpawn.OnTryDespawnClient()))
            {
                return false;
            }

            return true;
        }

        /// <summary>
        /// Reads a payload for a NetworkObject.
        /// </summary>
        internal void ReadPayload(NetworkConnection sender, NetworkObject nob, PooledReader reader, int? payloadLength = null)
        {
            if (!payloadLength.HasValue)
                payloadLength = (int)ReservedLengthWriter.ReadLength(reader, NetworkBehaviour.PAYLOAD_RESERVE_BYTES);
            // If there is a payload.
            if (payloadLength > 0)
            {
                if (nob != null)
                {
                    foreach (NetworkBehaviour networkBehaviour in nob.NetworkBehaviours)
                        networkBehaviour.ReadPayload(sender, reader);
                }
                // NetworkObject could be null if payload is for a predicted spawn.
                else
                {
                    reader.Skip((int)payloadLength);
                }
            }
        }

        /// <summary>
        /// Reads the payload returning it as an arraySegment.
        /// </summary>
        /// <returns></returns>
        internal ArraySegment<byte> ReadPayload(PooledReader reader)
        {
            int payloadLength = (int)ReservedLengthWriter.ReadLength(reader, NetworkBehaviour.PAYLOAD_RESERVE_BYTES);
            return reader.ReadArraySegment(payloadLength);
        }

        /// <summary>
        /// /Writers a payload for a NetworkObject.
        /// </summary>
        protected void WritePayload(NetworkConnection sender, NetworkObject nob, PooledWriter writer)
        {
            ReservedLengthWriter rw = ReservedWritersExtensions.Retrieve();

            rw.Initialize(writer, NetworkBehaviour.PAYLOAD_RESERVE_BYTES);

            foreach (NetworkBehaviour nb in nob.NetworkBehaviours)
                nb.WritePayload(sender, writer);

            rw.WriteLength();
        }

        // /// <summary>
        // /// Writes a payload for a NetworkObject.
        // /// </summary>
        // protected ArraySegment<byte> ReadPayload(PooledReader reader)
        // {
        //     PooledWriter nbWriter = WriterPool.Retrieve();
        //     foreach (NetworkBehaviour nb in nob.NetworkBehaviours)
        //     {
        //         nbWriter.Reset();
        //         nb.WritePayload(conn, nbWriter);
        //         if (nbWriter.Length > 0)
        //         {
        //             writer.WriteUInt8Unpacked(nb.ComponentIndex);
        //             writer.WriteArraySegment(nbWriter.GetArraySegment());
        //         }
        //     }
        // }
    }
}