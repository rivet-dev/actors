#if UNITY_EDITOR || DEVELOPMENT_BUILD
#define DEVELOPMENT
#endif
using FishNet.CodeGenerating;
using FishNet.Connection;
using FishNet.Documenting;
using FishNet.Managing;
using FishNet.Managing.Logging;
using FishNet.Managing.Predicting;
using FishNet.Managing.Server;
using FishNet.Managing.Timing;
using FishNet.Object.Prediction;
using FishNet.Object.Prediction.Delegating;
using FishNet.Serializing;
using FishNet.Serializing.Helping;
using FishNet.Transporting;
using FishNet.Utility;
using GameKit.Dependencies.Utilities;
using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using FishNet.Managing.Observing;
using GameKit.Dependencies.Utilities.Types;
using Unity.Profiling;
using UnityEngine;

[assembly: InternalsVisibleTo(UtilityConstants.CODEGEN_ASSEMBLY_NAME)]

namespace FishNet.Object
{
    #region Types.
    internal static class ReplicateTickFinder
    {
        public enum DataPlacementResult
        {
            /// <summary>
            /// Something went wrong; this should never be returned.
            /// </summary>
            Error,
            /// <summary>
            /// Tick was found on an index.
            /// </summary>
            Exact,
            /// <summary>
            /// Tick was not found because it is lower than any of the replicates.
            /// This is also used when there are no datas.
            /// </summary>
            InsertBeginning,
            /// <summary>
            /// Tick was not found but can be inserted in the middle of the collection.
            /// </summary>
            InsertMiddle,
            /// <summary>
            /// Tick was not found because it is larger than any of the replicates.
            /// </summary>
            InsertEnd
        }

        /// <summary>
        /// Gets the index in replicates where the tick matches.
        /// </summary>
        public static int GetReplicateHistoryIndex<T>(uint tick, RingBuffer<ReplicateDataContainer<T>> replicatesHistory, out DataPlacementResult findResult) where T : IReplicateData, new()
        {
            int replicatesCount = replicatesHistory.Count;
            if (replicatesCount == 0)
            {
                findResult = DataPlacementResult.InsertBeginning;
                return 0;
            }

            uint firstTick = replicatesHistory[0].Data.GetTick();

            // Try to find by skipping ahead the difference between tick and start.
            int diff = (int)(tick - firstTick);
            /* If the difference is larger than replicatesCount
             * then that means the replicates collection is missing
             * entries. EG if replicates values were 4, 7, 10 and tick were
             * 10 the difference would be 6. While replicates does contain the value
             * there is no way it could be found by pulling index 'diff' since that
             * would be out of bounds. This should never happen under normal conditions, return
             * missing if it does. */
            // Do not need to check less than 0 since we know if here tick is larger than first entry.
            if (diff >= replicatesCount)
            {
                // Try to return value using brute force.
                int index = FindIndexBruteForce(out findResult);
                return index;
            }

            if (diff < 0)
            {
                findResult = DataPlacementResult.InsertBeginning;
                return 0;
            }

            /* If replicatesHistory contained the ticks
             * of 1 2 3 4 5, and the tick is 3, then the difference
             * would be 2 (because 3 - 1 = 2). As we can see index
             * 2 of replicatesHistory does indeed return the proper tick. */
            // Expected diff to be result but was not.
            if (replicatesHistory[diff].Data.GetTick() != tick)
            {
                // Try to return value using brute force.
                int index = FindIndexBruteForce(out findResult);
                return index;
            }

            // Exact was found, this is the most ideal situation.
            findResult = DataPlacementResult.Exact;
            return diff;

            // Tries to find the index by brute forcing the collection.
            int FindIndexBruteForce(out DataPlacementResult result)
            {
                /* Some quick exits to save perf. */
                // If tick is lower than first then it must be inserted at the beginning.
                if (tick < firstTick)
                {
                    result = DataPlacementResult.InsertBeginning;
                    return 0;
                }

                // If tick is larger the last then it must be inserted at the end.
                if (tick > replicatesHistory[replicatesCount - 1].Data.GetTick())
                {
                    result = DataPlacementResult.InsertEnd;
                    return replicatesCount;
                }

                // Brute check.
                for (int i = 0; i < replicatesCount; i++)
                {
                    uint lTick = replicatesHistory[i].Data.GetTick();
                    // Exact match found.
                    if (lTick == tick)
                    {
                        result = DataPlacementResult.Exact;
                        return i;
                    }
                    /* The checked data is greater than
                     * what was being searched. This means
                     * to insert right before it. */

                    if (lTick > tick)
                    {
                        result = DataPlacementResult.InsertMiddle;
                        return i;
                    }
                }

                // Should be impossible to get here.
                result = DataPlacementResult.Error;
                return -1;
            }
        }
    }
    #endregion

    public abstract partial class NetworkBehaviour : MonoBehaviour
    {
        #region Public.
        /// <summary>
        /// True if this NetworkBehaviour is reconciling.
        /// If this NetworkBehaviour does not implement prediction methods this value will always be false.
        /// Value will be false if there is no data to reconcile to, even if the PredictionManager IsReconciling.
        /// Data may be missing if it were intentionally not sent, or due to packet loss.
        /// </summary>
        public bool IsBehaviourReconciling { get; internal set; }
        #endregion

        #region Internal.
        /// <summary>
        /// True if this NetworkBehaviour implements prediction methods.
        /// </summary>
        [APIExclude]
        internal bool UsesPrediction;
        /// <summary>
        /// True if the last read reconcile was remote, false if it was read from a LocalReconcile.
        /// </summary>
        internal bool IsReconcileRemote;
        #endregion

        #region Private.
        /// <summary>
        /// Registered Replicate methods.
        /// </summary>
        private Dictionary<uint, ReplicateRpcDelegate> _replicateRpcDelegates;
        /// <summary>
        /// Registered Reconcile methods.
        /// </summary>
        private Dictionary<uint, ReconcileRpcDelegate> _reconcileRpcDelegates;
        /// <summary>
        /// Number of replicate resends which may occur.
        /// </summary>
        private int _remainingReplicateResends;
        /// <summary>
        /// Number of reconcile resends which may occur.
        /// </summary>
        private int _remainingReconcileResends;
        /// <summary>
        /// Last replicate tick read from remote. This can be the server reading a client or the other way around.
        /// </summary>
        private uint _lastReplicateReadRemoteTick = TimeManager.UNSET_TICK;
        /// <summary>
        /// Tick when replicates should begun to run. This is set and used when inputs are just received and need to queue to create a buffer.
        /// </summary>
        private uint _replicateCurrentStartTick = TimeManager.UNSET_TICK;
        /// <summary>
        /// Last tick to replicate which was not replayed.
        /// </summary>
        private uint _lastOrderedReplicatedTick = TimeManager.UNSET_TICK;
        /// <summary>
        /// Last tick read for a replicate.
        /// </summary>
        private uint _lastReadReplicateTick = TimeManager.UNSET_TICK;
        /// <summary>
        /// Last tick read for a reconcile. This is only set on the client.
        /// </summary>
        private uint _lastReadReconcileRemoteTick = TimeManager.UNSET_TICK;
        /// <summary>
        /// Last tick this object reconciled on.
        /// </summary>
        private uint _lastReconcileTick = TimeManager.UNSET_TICK;
        /// <summary>
        /// Values from when the transform had changed last.
        /// </summary>
        /// <remarks>This is only used by prediction.</remarks>
        private TransformProperties _lastCheckedTransformProperties;
        #endregion

        /// <summary>
        /// Called when the object is destroyed.
        /// </summary>
        internal void OnDestroy_Prediction()
        {
            CollectionCaches<uint, ReplicateRpcDelegate>.StoreAndDefault(ref _replicateRpcDelegates);
            CollectionCaches<uint, ReconcileRpcDelegate>.StoreAndDefault(ref _reconcileRpcDelegates);
        }

        /// <summary>
        /// Registers a RPC method.
        /// Internal use.
        /// </summary>
        /// <param name = "hash"></param>
        /// <param name = "del"></param>
        [MakePublic]
        internal void RegisterReplicateRpc(uint hash, ReplicateRpcDelegate del)
        {
            UsesPrediction = true;

            if (_replicateRpcDelegates == null)
                _replicateRpcDelegates = CollectionCaches<uint, ReplicateRpcDelegate>.RetrieveDictionary();
            _replicateRpcDelegates[hash] = del;
        }

        /// <summary>
        /// Registers a RPC method.
        /// Internal use.
        /// </summary>
        /// <param name = "hash"></param>
        /// <param name = "del"></param>
        [MakePublic]
        internal void RegisterReconcileRpc(uint hash, ReconcileRpcDelegate del)
        {
            if (_reconcileRpcDelegates == null)
                _reconcileRpcDelegates = CollectionCaches<uint, ReconcileRpcDelegate>.RetrieveDictionary();
            _reconcileRpcDelegates[hash] = del;
        }

        /// <summary>
        /// Called when a replicate is received.
        /// </summary>
        internal void OnReplicateRpc(int readerPositionAfterDebug, uint? hash, PooledReader reader, NetworkConnection sendingClient, Channel channel)
        {
            if (hash == null)
                hash = ReadRpcHash(reader);

            reader.NetworkManager = _networkObjectCache.NetworkManager;

            if (_replicateRpcDelegates.TryGetValueIL2CPP(hash.Value, out ReplicateRpcDelegate del))
                del.Invoke(reader, sendingClient, channel);
            else
                _networkObjectCache.NetworkManager.LogWarning($"Replicate not found for hash {hash.Value} on {gameObject.name}, behaviour {GetType().Name}. Remainder of packet may become corrupt.");

            #if !UNITY_SERVER
            if (_networkTrafficStatistics != null)
            {
                bool sendingClientIsValid = sendingClient != null && sendingClient.IsValid;
                _networkTrafficStatistics.AddInboundPacketIdData(PacketId.Replicate, GetRpcName(PacketId.Replicate, hash.Value), reader.Position - readerPositionAfterDebug + Managing.Transporting.TransportManager.PACKETID_LENGTH, gameObject, asServer: sendingClientIsValid);
            }
            #endif
        }

        /// <summary>
        /// Called when a reconcile is received.
        /// </summary>
        internal void OnReconcileRpc(int readerPositionAfterDebug, uint? hash, PooledReader reader, Channel channel)
        {
            if (hash == null)
                hash = ReadRpcHash(reader);

            reader.NetworkManager = _networkObjectCache.NetworkManager;

            if (_reconcileRpcDelegates.TryGetValueIL2CPP(hash.Value, out ReconcileRpcDelegate del))
                del.Invoke(reader, channel);
            else
                _networkObjectCache.NetworkManager.LogWarning($"Reconcile not found for hash {hash.Value}. Remainder of packet may become corrupt.");

            #if !UNITY_SERVER
            if (_networkTrafficStatistics != null)
                _networkTrafficStatistics.AddInboundPacketIdData(PacketId.Reconcile, GetRpcName(PacketId.Reconcile, hash.Value), reader.Position - readerPositionAfterDebug + Managing.Transporting.TransportManager.PACKETID_LENGTH, gameObject, asServer: false);
            #endif
        }

        /// <summary>
        /// Resets cached ticks used by prediction, such as last read and replicate tick.
        /// This is generally used when the ticks will be different then what was previously used; eg: when ownership changes.
        /// </summary>
        private void ResetState_Prediction(bool asServer)
        {
            if (!asServer)
            {
                _lastReadReconcileRemoteTick = TimeManager.UNSET_TICK;
                _lastReconcileTick = TimeManager.UNSET_TICK;
            }

            _lastOrderedReplicatedTick = TimeManager.UNSET_TICK;
            _lastReplicateReadRemoteTick = TimeManager.UNSET_TICK;
            _lastReadReplicateTick = TimeManager.UNSET_TICK;

            ClearReplicateCache();
        }

        /// <summary>
        /// Clears cached replicates for server and client. This can be useful to call on server and client after teleporting.
        /// </summary>
        public virtual void ClearReplicateCache() { }

        /// <summary>
        /// Clears cached replicates and histories.
        /// </summary>
        [MakePublic]
        internal void ClearReplicateCache_Internal<T, T2>(BasicQueue<ReplicateDataContainer<T>> replicatesQueue, RingBuffer<ReplicateDataContainer<T>> replicatesHistory, RingBuffer<LocalReconcile<T2>> reconcilesHistory, ref T lastReadReplicate, ref T2 lastReadReconcile) where T : IReplicateData, new() where T2 : IReconcileData, new()
        {
            while (replicatesQueue.Count > 0)
            {
                ReplicateDataContainer<T> dataContainer = replicatesQueue.Dequeue();
                dataContainer.Dispose();
            }

            if (lastReadReplicate != null)
                lastReadReplicate.Dispose();
            lastReadReplicate = default;

            if (lastReadReconcile != null)
                lastReadReconcile.Dispose();
            lastReadReconcile = default;

            for (int i = 0; i < replicatesHistory.Count; i++)
            {
                ReplicateDataContainer<T> dataContainer = replicatesHistory[i];
                dataContainer.Dispose();
            }
            replicatesHistory.Clear();

            ClearReconcileHistory(reconcilesHistory, uint.MaxValue);
        }

        /// <summary>
        /// Sends a RPC to target.
        /// Internal use.
        /// </summary>
        [MakePublic]
        private void Server_SendReconcileRpc<T>(uint hash, ref T lastReconcileData, T reconcileData, Channel channel) where T : IReconcileData
        {
            if (!IsSpawned)
                return;

            // If channel is reliable set remaining resends to 1.
            if (channel == Channel.Reliable)
                _remainingReconcileResends = 1;

            if (_remainingReconcileResends == 0)
                return;
            _remainingReconcileResends--;

            // No owner and no state forwarding, nothing to do.
            bool stateForwarding = _networkObjectCache.EnableStateForwarding;
            if (!Owner.IsValid && !stateForwarding)
                return;

            /* Set the channel for Rpcs to reliable to that the length
             * is written. The data does not actually send reliable, unless
             * the channel is of course that to start. */
            /* This is a temporary solution to resolve an issue which was
             * causing parsing problems due to states sending unreliable and reliable
             * headers being written, or sending reliably and unreliable headers being written.
             * Using an extra byte to write length is more preferred than always forcing reliable
             * until properly resolved. */
            const Channel rpcChannel = Channel.Reliable;

            PooledWriter methodWriter = WriterPool.Retrieve();
            /* Tick does not need to be written because it will always
             * be the localTick of the server. For the clients, this will
             * be the LastRemoteTick of the packet.
             *
             * The exception is for the owner, which we send the last replicate
             * tick so the owner knows which to roll back to. */
            #if DO_NOT_USE
            methodWriter.WriteDeltaReconcile(lastReconcileData, reconcileData, GetDeltaSerializeOption());
            #else
            methodWriter.WriteReconcile<T>(reconcileData);
            #endif
            lastReconcileData = reconcileData;

            PooledWriter writer;
            #if DEVELOPMENT
            if (!NetworkManager.DebugManager.DisableReconcileRpcLinks && _rpcLinks.TryGetValueIL2CPP(hash, out RpcLinkType link))
                #else
            if (_rpcLinks.TryGetValueIL2CPP(hash, out RpcLinkType link))
                #endif
                writer = CreateLinkedRpc(link, methodWriter, rpcChannel);
            else
                writer = CreateRpc(hash, methodWriter, PacketId.Reconcile, rpcChannel);

            #if !UNITY_SERVER
            int observersWrittenTo = 0;
            #endif

            // If state forwarding is not enabled then only send to owner.
            if (!stateForwarding)
            {
                Owner.WriteState(writer);
            }
            // State forwarding, send to all.
            else
            {
                {
                    #if !UNITY_SERVER
                    observersWrittenTo = Observers.Count;
                    #endif

                    //Send to everyone unconditionally when not using LOD.
                    foreach (NetworkConnection nc in Observers)
                        nc.WriteState(writer);
                }
            }

            #if !UNITY_SERVER
            if (_networkTrafficStatistics != null)
            {
                int writtenBytes = stateForwarding ? writer.Length * observersWrittenTo : writer.Length;
                _networkTrafficStatistics.AddOutboundPacketIdData(PacketId.Reconcile, GetRpcName(PacketId.Reconcile, hash), writtenBytes + Managing.Transporting.TransportManager.PACKETID_LENGTH, gameObject, asServer: true);
            }
            #endif

            methodWriter.Store();
            writer.Store();
        }

        /// <summary>
        /// Returns if there is a chance the transform may change after the tick.
        /// </summary>
        /// <returns></returns>
        private bool HasServerRigidbodyTransformChanged(bool updateLastValues)
        {
            if (!IsServerStarted)
                return false;

            if (TimeManager.PhysicsMode == PhysicsMode.Disabled)
                return false;

            /* Use distance when checking if changed because rigidbodies can twitch
             * or move a minimal amount. These small moves are not worth
             * resending over because they often fix themselves each frame. */

            const float v3Distance = 0.000004f;
            const float angleDistance = 0.2f;

            bool anyChanged = false;
            anyChanged |= (transform.position - _lastCheckedTransformProperties.Position).sqrMagnitude > v3Distance;
            if (!anyChanged)
                anyChanged |= transform.rotation.Angle(_lastCheckedTransformProperties.Rotation, precise: true) > angleDistance;
            if (!anyChanged)
                anyChanged |= (transform.localScale - _lastCheckedTransformProperties.Scale).sqrMagnitude > v3Distance;

            // If transform changed update last values.
            if (updateLastValues && anyChanged)
                _lastCheckedTransformProperties.Update(transform);

            return anyChanged;
        }

        /// <summary>
        /// Performs a replicate for current tick.
        /// </summary>
        [MakePublic]
        internal void Replicate_Current<T>(ReplicateUserLogicDelegate<T> del, uint methodHash, BasicQueue<ReplicateDataContainer<T>> replicatesQueue, RingBuffer<ReplicateDataContainer<T>> replicatesHistory, ReplicateDataContainer<T> dataContainer) where T : IReplicateData, new()
        {
            /* Do not run if currently reconciling.
             * This change allows devs to call inherited replicates
             * from replays to only run the method logic without
             * prompting for network action. */
            if (_networkObjectCache.PredictionManager.IsReconciling)
                return;

            if (_networkObjectCache.IsController)
                Replicate_Authoritative(del, methodHash, replicatesHistory, dataContainer);
            else
                Replicate_NonAuthoritative(del, replicatesQueue, replicatesHistory);
        }

        /// <summary>
        /// Returns if a replicates data changed and updates resends as well data tick.
        /// </summary>
        /// <param name = "enqueueData">True to enqueue data for replaying.</param>
        /// <returns>True if data has changed..</returns>
        private void Replicate_Authoritative<T>(ReplicateUserLogicDelegate<T> del, uint methodHash, RingBuffer<ReplicateDataContainer<T>> replicatesHistory, ReplicateDataContainer<T> dataContainer) where T : IReplicateData, new()
        {
            bool ownerlessAndServer = !Owner.IsValid && IsServerStarted;
            if (!IsOwner && !ownerlessAndServer)
                return;

            Func<T, bool> isDefaultDel = PublicPropertyComparer<T>.IsDefault;
            if (isDefaultDel == null)
            {
                NetworkManager.LogError($"{nameof(PublicPropertyComparer<T>)} not found for type {typeof(T).FullName}");
                return;
            }

            PredictionManager pm = NetworkManager.PredictionManager;
            uint dataTick = TimeManager.LocalTick;

            /* The following code is to remove replicates from replicatesHistory
             * which exceed the buffer allowance. Replicates are kept for up to
             * x seconds to clients can re-run them during a reconcile. The reconcile
             * method removes old histories but given the server does not reconcile,
             * it will never perform that operation.
             * The server would not actually need to keep replicates history except
             * when it is also client(clientHost). This is because the clientHost must
             * send redundancies to other clients still, therefor that redundancyCount
             * must be the allowance when clientHost. */
            if (IsHostStarted)
            {
                int replicatesHistoryCount = replicatesHistory.Count;
                int maxCount = pm.RedundancyCount;
                // Number to remove which is over max count.
                int removeCount = replicatesHistoryCount - maxCount;
                // If there are any to remove.
                if (removeCount > 0)
                {
                    // Dispose first.
                    for (int i = 0; i < removeCount; i++)
                        replicatesHistory[i].Dispose();

                    // Then remove range.
                    replicatesHistory.RemoveRange(true, removeCount);
                }
            }

            dataContainer.SetDataTick(dataTick);
            AddReplicatesHistory(replicatesHistory, dataContainer);

            // Check to reset resends.
            bool isDefault = isDefaultDel.Invoke(dataContainer.Data);
            bool resetResends = !isDefault || HasServerRigidbodyTransformChanged(updateLastValues: true);

            byte redundancyCount = PredictionManager.RedundancyCount;

            // Standard delta serialize option.
            // +1 to redundancy so lastFirstRead is pushed out to the last actual input when server reads.
            if (resetResends)
            {
                _remainingReplicateResends = redundancyCount;
                _remainingReconcileResends = redundancyCount;
            }

            bool sendData = _remainingReplicateResends > 0;
            if (sendData)
            {
                /* If not server then send to server.
                 * If server then send to clients. */
                bool toServer = !IsServerStarted;
                Replicate_SendAuthoritative(toServer, methodHash, redundancyCount, replicatesHistory, dataTick, dataContainer.Channel, GetDeltaSerializeOption());
                _remainingReplicateResends--;
            }

            SetReplicateTick(dataTick, createdReplicate: true);

            // Owner always replicates with new data.
            del.Invoke(dataContainer.Data, ReplicateState.Ticked | ReplicateState.Created, dataContainer.Channel);
        }

        /// <summary>
        /// Gets the next replicate in perform when server or non-owning client.
        /// </summary>
        /// </summary>
        private void Replicate_NonAuthoritative<T>(ReplicateUserLogicDelegate<T> del, BasicQueue<ReplicateDataContainer<T>> replicatesQueue, RingBuffer<ReplicateDataContainer<T>> replicatesHistory) where T : IReplicateData, new()
        {
            PredictionManager predictionManager = PredictionManager;

            bool isServerStarted = _networkObjectCache.IsServerStarted;
            bool isServerWithoutOwner = isServerStarted && !Owner.IsValid;

            /* Both owner and server when no owner should run
             * authoritative replicate. */
            if (isServerWithoutOwner)
                return;
            /* If not state forwarding and not server then exit method.
             * The server still needs to run inputs even if not authoritative. */
            if (!isServerStarted && !_networkObjectCache.EnableStateForwarding)
                return;

            TimeManager tm = _networkObjectCache.TimeManager;
            uint localTick = tm.LocalTick;

            /* If not appended order and not server then
             * run default input and exit. With inserted order client only runs
             * during replays. Server never replays, so it still runs
             * as current even with inserted order. */
            if (!isServerStarted && !predictionManager.IsAppendedStateOrder)
            {
                ReplicateDefaultData();
                return;
            }

            int count = replicatesQueue.Count;
            /* If count is 0 then data must be set default
             * and as predicted. */
            if (count == 0)
            {
                if (HasServerRigidbodyTransformChanged(updateLastValues: true))
                    _remainingReconcileResends = predictionManager.RedundancyCount;

                ReplicateDefaultData();
            }
            //Not predicted, is user created.
            else
            {
                //Check to unset start tick, which essentially voids it resulting in inputs being run immediately.
                if (localTick >= _replicateCurrentStartTick)
                {
                    //_replicateCurrentStartTick = TimeManager.UNSET_TICK;
                    _replicateCurrentStartTick = TimeManager.UNSET_TICK;
                    ReplicateDataContainer<T> queueEntry;
                    bool queueEntryValid = false;

                    if (!predictionManager.IsAppendedStateOrder)
                    {
                        while (replicatesQueue.TryDequeue(out queueEntry))
                        {
                            if (queueEntry.Data.GetTick() > _lastReconcileTick)
                            {
                                queueEntryValid = true;
                                break;
                            }
                        }
                    }
                    else
                    {
                        queueEntryValid = replicatesQueue.TryDequeue(out queueEntry);
                    }

                    if (queueEntryValid)
                    {
                        _remainingReconcileResends = predictionManager.RedundancyCount;

                        ReplicateData(queueEntry, ReplicateState.Ticked | ReplicateState.Created);

                        //Update count since old entries were dropped and one replicate run.
                        count = replicatesQueue.Count;

                        bool consumeExcess = !predictionManager.DropExcessiveReplicates || IsClientOnlyStarted;
                        int leaveInBuffer = _networkObjectCache.PredictionManager.StateInterpolation;

                        //Only consume if the queue count is over leaveInBuffer.
                        if (consumeExcess && count > leaveInBuffer)
                        {
                            const byte maximumAllowedConsumes = 1;
                            int maximumPossibleConsumes = count - leaveInBuffer;
                            int consumeAmount = Mathf.Min(maximumAllowedConsumes, maximumPossibleConsumes);

                            for (int i = 0; i < consumeAmount; i++)
                                ReplicateData(replicatesQueue.Dequeue(), ReplicateState.Ticked | ReplicateState.Created);
                        }
                    }
                    else
                    {
                        ReplicateDefaultData();
                    }
                }
                else
                {
                    ReplicateDefaultData();
                }
            }

            //Performs a replicate using default data.
            void ReplicateDefaultData()
            {
                /* If it has been awhile since this properly has updated then
                 * use the connections estimated tick with interpolation. */
                uint estimatedTick;
                if (IsServerStarted)
                    estimatedTick = Owner.ReplicateTick.IsUnset ? Owner.PacketTick.Value() + predictionManager.StateInterpolation : Owner.ReplicateTick.Value();
                else
                    estimatedTick = tm.LastPacketTick.Value() - predictionManager.StateInterpolation;
                //estimatedTick = tm.LocalTick; // + predictionManager.StateInterpolation;// tm.LastPacketTick.Value() + predictionManager.StateInterpolation;

                ReplicateDataContainer<T> dataContainer = ReplicateDataContainer<T>.GetDefault(estimatedTick);
                ReplicateData(dataContainer, ReplicateState.Ticked);
            }

            void ReplicateData(ReplicateDataContainer<T> data, ReplicateState state)
            {
                // if (!isServerStarted)
                //     data.Data.SetTick(tm.LocalTick);

                uint tick = data.Data.GetTick();

                SetReplicateTick(tick, state.ContainsCreated());

                //Server always adds.
                if (IsServerStarted)
                    AddReplicatesHistory(replicatesHistory, data);
                //If client insert value into history.
                else
                    InsertIntoReplicateHistory(data, replicatesHistory);

                del.Invoke(data.Data, state, data.Channel);
            }
        }

        /// <summary>
        /// Called internally when an input from localTick should be replayed.
        /// </summary>
        internal virtual void Replicate_Replay_Start(uint replayTick) { }

        /// <summary>
        /// Replays inputs from replicates.
        /// </summary>
        /// <remarks>The server calls this from codegen but it never completes as IsBehaviourReconciling will always be false on server.</remarks>
        [MakePublic]
        internal void Replicate_Replay<T>(uint replayTick, ReplicateUserLogicDelegate<T> del, RingBuffer<ReplicateDataContainer<T>> replicatesHistory) where T : IReplicateData, new()
        {
            //Reconcile data was not received so cannot replay.
            if (!IsBehaviourReconciling)
                return;

            if (_networkObjectCache.IsController)
                Replicate_Replay_Authoritative(replayTick, del, replicatesHistory);
            else
                Replicate_Replay_NonAuthoritative(replayTick, del, replicatesHistory);
        }

        /// <summary>
        /// Replays an input for authoritative entity.
        /// </summary>
        private void Replicate_Replay_Authoritative<T>(uint replayTick, ReplicateUserLogicDelegate<T> del, RingBuffer<ReplicateDataContainer<T>> replicatesHistory) where T : IReplicateData, new()
        {
            ReplicateTickFinder.DataPlacementResult findResult;
            int replicateIndex = ReplicateTickFinder.GetReplicateHistoryIndex(replayTick, replicatesHistory, out findResult);

            ReplicateDataContainer<T> dataContainer;
            ReplicateState state;
            //If found then the replicate has been received by the server.
            if (findResult == ReplicateTickFinder.DataPlacementResult.Exact)
            {
                dataContainer = replicatesHistory[replicateIndex];
                state = ReplicateState.Replayed | ReplicateState.Ticked | ReplicateState.Created;

                //SetReplicateTick(data.GetTick(), true);
                del.Invoke(dataContainer.Data, state, dataContainer.Channel);
            }
        }

        /// <summary>
        /// Replays an input for non authoritative entity.
        /// </summary>
        [MakePublic]
        private void Replicate_Replay_NonAuthoritative<T>(uint replayTick, ReplicateUserLogicDelegate<T> del, RingBuffer<ReplicateDataContainer<T>> replicatesHistory) where T : IReplicateData, new()
        {
            ReplicateDataContainer<T> dataContainer;
            ReplicateState state;
            bool isAppendedOrder = _networkObjectCache.PredictionManager.IsAppendedStateOrder;
            //If the first replay.
            if (isAppendedOrder || replayTick == _networkObjectCache.PredictionManager.ServerStateTick + 1)
            {
                ReplicateTickFinder.DataPlacementResult findResult;
                int replicateIndex = ReplicateTickFinder.GetReplicateHistoryIndex(replayTick, replicatesHistory, out findResult);
                //If not found then something went wrong.
                if (findResult == ReplicateTickFinder.DataPlacementResult.Exact)
                {
                    dataContainer = replicatesHistory[replicateIndex];

                    state = ReplicateState.Replayed;

                    bool isCreated = dataContainer.IsCreated;
                    //Set if created.
                    if (isCreated)
                        state |= ReplicateState.Created;
                    /* Ticked will be true if value had ticked outside of reconcile,
                     * or if data is created. It's possible for data to be created
                     * and not yet ticked if state order is inserted rather than append. */
                    if (replayTick <= _lastOrderedReplicatedTick || isCreated)
                        state |= ReplicateState.Ticked;
                }
                else
                {
                    SetDataToDefault();
                }
            }
            //Not the first replay tick.
            else
            {
                SetDataToDefault();
            }

            void SetDataToDefault()
            {
                dataContainer = ReplicateDataContainer<T>.GetDefault(replayTick);
                state = ReplicateState.Replayed;
            }

            del.Invoke(dataContainer.Data, state, dataContainer.Channel);
        }

        /// <summary>
        /// This is overriden by codegen to call EmptyReplicatesQueueIntoHistory().
        /// This should only be called when client only.
        /// </summary>
        [MakePublic]
        internal virtual void EmptyReplicatesQueueIntoHistory_Start() { }

        /// <summary>
        /// Replicates which are enqueued will be removed from the queue and put into replicatesHistory.
        /// This should only be called when client only.
        /// </summary>
        [MakePublic]
        internal void EmptyReplicatesQueueIntoHistory<T>(BasicQueue<ReplicateDataContainer<T>> replicatesQueue, RingBuffer<ReplicateDataContainer<T>> replicatesHistory) where T : IReplicateData, new()
        {
            while (replicatesQueue.TryDequeue(out ReplicateDataContainer<T> data))
                InsertIntoReplicateHistory(data, replicatesHistory);
        }

        /// <summary>
        /// Returns the DeltaSerializeOption to use for the tick.
        /// </summary>
        /// <param name = "resendsEnded"></param>
        /// <returns></returns>
        private DeltaSerializerOption GetDeltaSerializeOption()
        {
            //Everything below this is not yet used.
            return DeltaSerializerOption.FullSerialize;
            //
            // uint localTick = _networkObjectCache.TimeManager.LocalTick;
            // ushort tickRate = _networkObjectCache.TimeManager.TickRate;
            // /* New observers so send a full serialize next replicate.
            //  * This could go out to only the newly added observers, but it
            //  * would generate a lot more complexity to save presumably
            //  * a small amount of occasional bandwidth. */
            // if (_networkObjectCache.ObserverAddedTick == localTick)
            //     return DeltaSerializerOption.FullSerialize;
            // //Send full every half a second.
            // if (localTick % tickRate == 0 || localTick % (tickRate / 2) == 0)
            //     return DeltaSerializerOption.FullSerialize;
            // //Send full every second.
            // if (localTick % tickRate == 0)
            //     return DeltaSerializerOption.FullSerialize;
            // //Otherwise return rootSerialize, the default for sending the child most data.
            //
            // return DeltaSerializerOption.RootSerialize;
        }

        /// <summary>
        /// Sends a Replicate to server or clients.
        /// </summary>
        private void Replicate_SendAuthoritative<T>(bool toServer, uint hash, int pastInputs, RingBuffer<ReplicateDataContainer<T>> replicatesHistory, uint queuedTick, Channel channel, DeltaSerializerOption deltaOption) where T : IReplicateData, new()
        {
            /* Do not use IsSpawnedWithWarning because the server
             * will still call this a tick or two as clientHost when
             * an owner disconnects. This comes from calling Replicate(default)
             * for the server-side processing in NetworkBehaviours. */
            if (!IsSpawned)
                return;

            int historyCount = replicatesHistory.Count;
            //Nothing to send; should never be possible.
            if (historyCount <= 0)
                return;

            //Number of past inputs to send.
            if (historyCount < pastInputs)
                pastInputs = historyCount;
            /* Where to start writing from. When passed
             * into the writer values from this offset
             * and forward will be written.
             * Always write up to past inputs. */
            int offset = historyCount - pastInputs;

            //Write history to methodWriter.
            PooledWriter methodWriter = WriterPool.Retrieve(WriterPool.LENGTH_BRACKET);
            /* If going to clients from the server then
             * write the queueTick. */
            if (!toServer)
                methodWriter.WriteTickUnpacked(queuedTick);
            #if DO_NOT_USE
            methodWriter.WriteDeltaReplicate(replicatesHistory, offset, deltaOption);
            #else
            methodWriter.WriteReplicate<T>(replicatesHistory, offset);
            #endif

            channel = _transportManagerCache.GetReliableChannelIfOverMTU(methodWriter.Length + MAXIMUM_RPC_HEADER_SIZE, channel);

            PooledWriter writer = CreateRpc(hash, methodWriter, PacketId.Replicate, channel);

            #if !UNITY_SERVER
            int written = 0;
            #endif
            /* toServer will never be true if clientHost.
             * When clientHost and here replicates will
             * always just send to clients, while
             * excluding clientHost. */
            if (toServer)
            {
                #if !UNITY_SERVER
                written = writer.Length;
                #endif
                NetworkManager.TransportManager.SendToServer((byte)channel, writer.GetArraySegment());
            }
            else
            {
                /* If going to clients from server, then only send
                 * if state forwarding is enabled. */
                if (_networkObjectCache.EnableStateForwarding)
                {
                    //Exclude owner and if clientHost, also localClient.
                    _networkConnectionCache.Clear();
                    _networkConnectionCache.Add(Owner);
                    if (IsClientStarted)
                        _networkConnectionCache.Add(ClientManager.Connection);

                    #if !UNITY_SERVER
                    written = writer.Length * (Observers.Count - _networkConnectionCache.Count);
                    #endif
                    NetworkManager.TransportManager.SendToClients((byte)channel, writer.GetArraySegment(), Observers, _networkConnectionCache);
                }
            }

            #if !UNITY_SERVER
            if (written != 0 && _networkTrafficStatistics != null)
                _networkTrafficStatistics.AddOutboundPacketIdData(PacketId.Replicate, GetRpcName(PacketId.Replicate, hash), written, gameObject, asServer: true);
            #endif

            /* If sending as reliable there is no reason
             * to perform resends, so clear remaining resends. */
            if (channel == Channel.Reliable)
                _remainingReplicateResends = 0;

            methodWriter.StoreLength();
            writer.StoreLength();
        }

        /// <summary>
        /// Reads a replicate the client.
        /// </summary>
        [MakePublic]
        internal void Replicate_Reader<T>(uint hash, PooledReader reader, NetworkConnection sender, ref ReplicateDataContainer<T> lastReadReplicate, BasicQueue<ReplicateDataContainer<T>> replicatesQueue, RingBuffer<ReplicateDataContainer<T>> replicatesHistory, Channel channel) where T : IReplicateData, new()
        {
            /* This will never be received on owner, except in the condition
             * the server is the owner and also a client. In such condition
             * the method is exited after data is parsed. */
            PredictionManager pm = _networkObjectCache.PredictionManager;
            TimeManager tm = _networkObjectCache.TimeManager;
            bool fromServer = reader.Source == Reader.DataSource.Server;

            uint tick;
            /* If coming from the server then read the tick. Server sends tick
             * if authority or if relaying from another client. The tick which
             * arrives will be the tick the replicate will run on the server. */
            if (fromServer)
                tick = reader.ReadTickUnpacked();
            /* When coming from a client it will always be owner.
             * Client sends out replicates soon as they are run.
             * It's safe to use the LastRemoteTick from the client
             * in addition to QueuedInputs. */
            else
                tick = tm.LastPacketTick.LastRemoteTick;

            #if DO_NOT_USE
            receivedReplicatesCount = reader.ReadDeltaReplicate(lastReadReplicate, ref arrBuffer, tick);
            #else
            List<ReplicateDataContainer<T>> readReplicates = reader.ReadReplicate<T>(tick);
            #endif


            /* This does not need to log to statistics -- network traffic
             * will be read when the state executes. */

            //Update first read if able.
            if (readReplicates.Count > 0)
            {
                lastReadReplicate.Dispose();
                lastReadReplicate = readReplicates[^1];
            }

            //If received on clientHost simply ignore after parsing data.
            if (fromServer && IsHostStarted)
                return;

            /* Replicate rpc readers relay to this method and
             * do not have an owner check in the generated code.
             * Only server needs to check for owners. Clients
             * should accept the servers data regardless.
             *
             * If coming from a client and that client is not owner then exit. */
            if (!fromServer && !OwnerMatches(sender))
                return;
            //Early exit if old data.
            if (TimeManager.LastPacketTick.LastRemoteTick < _lastReplicateReadRemoteTick)
                return;
            _lastReplicateReadRemoteTick = TimeManager.LastPacketTick.LastRemoteTick;

            //If from a client that is not clientHost do some safety checks.
            if (!fromServer && !Owner.IsLocalClient)
            {
                if (readReplicates.Count > pm.RedundancyCount)
                {
                    sender.Kick(reader, KickReason.ExploitAttempt, LoggingType.Common, $"Connection {sender.ToString()} sent too many past replicates. Connection will be kicked immediately.");
                    return;
                }
            }

            Replicate_EnqueueReceivedReplicate(readReplicates, replicatesQueue, replicatesHistory);
            Replicate_SendNonAuthoritative(hash, replicatesQueue, channel);

            CollectionCaches<ReplicateDataContainer<T>>.Store(readReplicates);
        }

        /// <summary>
        /// Sends data from a reader which only contains the replicate packet.
        /// </summary>
        [MakePublic]
        internal void Replicate_SendNonAuthoritative<T>(uint hash, BasicQueue<ReplicateDataContainer<T>> replicatesQueue, Channel channel) where T : IReplicateData, new()
        {
            if (!IsServerStarted)
                return;
            if (!_networkObjectCache.EnableStateForwarding)
                return;

            int queueCount = replicatesQueue.Count;
            //None to send.
            if (queueCount == 0)
                return;

            //If the only observer is the owner then there is no need to write.
            int observersCount = Observers.Count;
            //Quick exit for no observers other than owner.
            if (observersCount == 0 || (Owner.IsValid && observersCount == 1))
                return;

            PooledWriter methodWriter = WriterPool.Retrieve(WriterPool.LENGTH_BRACKET);

            uint localTick = _networkObjectCache.TimeManager.LocalTick;
            /* Write when the last entry will run.
             *
             * Typically, the last entry will run on localTick + (queueCount - 1).
             * 1 is subtracted from queueCount because in most cases the first entry
             * is going to run same tick.
             * An exception is when the replicateStartTick is set, then there is going
             * to be a delayed based on start tick difference. */
            uint runTickOflastEntry = localTick + ((uint)queueCount - 1);
            //If start tick is set then add on the delay.
            if (_replicateCurrentStartTick != TimeManager.UNSET_TICK)
                runTickOflastEntry += _replicateCurrentStartTick - TimeManager.LocalTick;
            //Write the run tick now.
            methodWriter.WriteTickUnpacked(runTickOflastEntry);
            //Write the replicates.
            int redundancyCount = (int)Mathf.Min(_networkObjectCache.PredictionManager.RedundancyCount, queueCount);
            #if DO_NOT_USE
            methodWriter.WriteDeltaReplicate(replicatesQueue, redundancyCount, GetDeltaSerializeOption());
            #else
            methodWriter.WriteReplicate<T>(replicatesQueue, redundancyCount);
            #endif
            PooledWriter writer = CreateRpc(hash, methodWriter, PacketId.Replicate, channel);

            //Exclude owner and if clientHost, also localClient.
            _networkConnectionCache.Clear();
            if (Owner.IsValid)
                _networkConnectionCache.Add(Owner);
            if (IsClientStarted && !Owner.IsLocalClient)
                _networkConnectionCache.Add(ClientManager.Connection);

            #if !UNITY_SERVER
            if (_networkTrafficStatistics != null)
            {
                int written = writer.Length * (Observers.Count - _networkConnectionCache.Count);
                _networkTrafficStatistics.AddOutboundPacketIdData(PacketId.Replicate, GetRpcName(PacketId.Replicate, hash), written, gameObject, asServer: true);
            }
            #endif

            NetworkManager.TransportManager.SendToClients((byte)channel, writer.GetArraySegment(), Observers, _networkConnectionCache);

            methodWriter.StoreLength();
            writer.StoreLength();
        }

        /// <summary>
        /// Handles a received replicate packet.
        /// </summary>
        private void Replicate_EnqueueReceivedReplicate<T>(List<ReplicateDataContainer<T>> readDatas, BasicQueue<ReplicateDataContainer<T>> replicatesQueue, RingBuffer<ReplicateDataContainer<T>> replicatesHistory) where T : IReplicateData, new()
        {
            int startQueueCount = replicatesQueue.Count;
            /* Owner never gets this for their own object so
             * this can be processed under the assumption data is only
             * handled on unowned objects. */
            PredictionManager pm = PredictionManager;

            bool isServer = _networkObjectCache.IsServerStarted;
            bool isAppendedOrder = pm.IsAppendedStateOrder;

            //Maximum number of replicates allowed to be queued at once.
            int maximumReplicates = IsServerStarted ? pm.GetMaximumServerReplicates() : pm.MaximumPastReplicates;

            for (int i = 0; i < readDatas.Count; i++)
            {
                ReplicateDataContainer<T> dataContainer = readDatas[i];
                dataContainer.IsCreated = true;
                uint tick = dataContainer.Data.GetTick();

                //Skip if old data.
                if (tick <= _lastReadReplicateTick)
                {
                    dataContainer.Dispose();
                    continue;
                }

                _lastReadReplicateTick = tick;

                //Cannot queue anymore, discard oldest.
                if (replicatesQueue.Count > maximumReplicates)
                {
                    ReplicateDataContainer<T> disposableDataContainer = replicatesQueue.Dequeue();
                    disposableDataContainer.Dispose();
                }

                /* Check if replicate is already in history.
                 * This can occur when the replicate method has a predicted
                 * state for the tick, but a user created replicate comes
                 * through afterward.
                 *
                 * Only perform this check if not the server, since server
                 * does not reconcile it will never use replicatesHistory.
                 *
                 * When clients are also using ReplicateStateOrder.Future the replicates
                 * do not need to be put into the past, as they're always added onto
                 * the end of the queue.
                 *
                 * The server also does not predict replicates in the same way
                 * a client does. When an owner sends a replicate to the server
                 * the server only uses the owner tick to check if it's an old replicate.
                 * But when running the replicate, the server applies it's local tick and
                 * sends that to spectators. */
                //Add automatically if server or future order.
                if (isServer || isAppendedOrder)
                    replicatesQueue.Enqueue(dataContainer);
                //Run checks to replace data if not server.
                else
                    InsertIntoReplicateHistory(dataContainer, replicatesHistory);
            }

            /* If entries are being added after nothing then
             * start the queued inputs delay. Only the server needs
             * to do this since clients implement the queue delay
             * by holding reconcile x ticks rather than not running received
             * x ticks. */
            if (_replicateCurrentStartTick != TimeManager.UNSET_TICK && (isServer || isAppendedOrder) && startQueueCount == 0 && replicatesQueue.Count > 0)
                _replicateCurrentStartTick = _networkObjectCache.TimeManager.LocalTick + pm.StateInterpolation;
        }

        /// <summary>
        /// Inserts data into the replicatesHistory collection.
        /// This should only be called when client only.
        /// </summary>
        private void InsertIntoReplicateHistory<T>(ReplicateDataContainer<T> dataContainer, RingBuffer<ReplicateDataContainer<T>> replicatesHistory) where T : IReplicateData, new()
        {
            /* See if replicate tick is in history. Keep in mind
             * this is the localTick from the server, not the localTick of
             * the client which is having their replicate relayed. */
            ReplicateTickFinder.DataPlacementResult findResult;
            int index = ReplicateTickFinder.GetReplicateHistoryIndex(dataContainer.Data.GetTick(), replicatesHistory, out findResult);

            /* Exact entry found. This is the most likely
             * scenario. Client would have already run the tick
             * in the future, and it's now being replaced with
             * the proper data. */
            if (findResult == ReplicateTickFinder.DataPlacementResult.Exact)
            {
                ReplicateDataContainer<T> prevEntry = replicatesHistory[index];
                prevEntry.Dispose();
                replicatesHistory[index] = dataContainer;
            }
            else if (findResult == ReplicateTickFinder.DataPlacementResult.InsertMiddle)
            {
                InsertReplicatesHistory(replicatesHistory, dataContainer, index);
            }
            else if (findResult == ReplicateTickFinder.DataPlacementResult.InsertEnd)
            {
                AddReplicatesHistory(replicatesHistory, dataContainer);
            }

            /* Insert beginning should not happen unless the data is REALLY old.
             * This would mean the network was in an unplayable state. Discard the
             * data. */
            if (findResult == ReplicateTickFinder.DataPlacementResult.InsertBeginning)
                InsertReplicatesHistory(replicatesHistory, dataContainer, 0);
        }

        /// <summary>
        /// Adds to replicate history disposing of old entries if needed.
        /// </summary>
        private void AddReplicatesHistory<T>(RingBuffer<ReplicateDataContainer<T>> replicatesHistory, ReplicateDataContainer<T> value) where T : IReplicateData, new()
        {
            ReplicateDataContainer<T> prev = replicatesHistory.Add(value);
            if (prev.Data != null)
                prev.Dispose();
        }

        /// <summary>
        /// Inserts to replicate history disposing of old entries if needed.
        /// </summary>
        private void InsertReplicatesHistory<T>(RingBuffer<ReplicateDataContainer<T>> replicatesHistory, ReplicateDataContainer<T> value, int index) where T : IReplicateData, new()
        {
            ReplicateDataContainer<T> prev = replicatesHistory.Insert(index, value);
            if (prev.Data != null)
                prev.Dispose();
        }

        /// <summary>
        /// Override this method to create your reconcile data, and call your reconcile method.
        /// </summary>
        public virtual void CreateReconcile() { }

        /// <summary>
        /// Sends a reconcile to clients.
        /// </summary>
        [MakePublic]
        internal void Reconcile_Server<T>(uint methodHash, ref T lastReconcileData, T data, Channel channel) where T : IReconcileData
        {
            //Tick does not need to be set for reconciles since they come in as state updates, which have the tick included globally.
            if (IsServerInitialized)
                Server_SendReconcileRpc(methodHash, ref lastReconcileData, data, channel);
        }

        /// <summary>
        /// This is called when the NetworkBehaviour should perform a reconcile.
        /// Codegen overrides this calling Reconcile_Client with the needed data.
        /// </summary>
        [MakePublic]
        internal virtual void Reconcile_Client_Start() { }

        /// <summary>
        /// Adds a reconcile to local reconcile history.
        /// </summary>
        [MakePublic]
        internal void Reconcile_Client_AddToLocalHistory<T>(RingBuffer<LocalReconcile<T>> reconcilesHistory, T data) where T : IReconcileData
        {
            //Server does not need to store these locally.
            if (_networkObjectCache.IsServerStarted || !_networkObjectCache.PredictionManager.CreateLocalStates)
                return;
            if (!IsOwner && !_networkObjectCache.EnableStateForwarding)
                return;

            /* This is called by the local client when creating
             * a local reconcile state. These states should always
             * be in order, so we will add data to the end
             * of the collection. */

            /* These datas are used to fill missing reconciles
             * be it the packet dropped, server doesnt need to send,
             * or if the player is throttling reconciles. */

            uint tick; // =  _networkObjectCache.PredictionManager.GetCreateReconcileTick();//_networkObjectCache.IsOwner);

            tick = _networkObjectCache.TimeManager.LocalTick;

            //Tick couldn't be retrieved.
            if (tick == TimeManager.UNSET_TICK)
                return;

            data.SetTick(tick);

            //Build LocalReconcile.
            LocalReconcile<T> lr = new();
            lr.Initialize(tick, data);

            reconcilesHistory.Add(lr);
        }

        /// <summary>
        /// Called by codegen with data provided by user, such as from overriding CreateReconcile.
        /// </summary>
        [MakePublic]
        internal void Reconcile_Current<T>(uint hash, ref T lastReconcileData, RingBuffer<LocalReconcile<T>> reconcilesHistory, T data, Channel channel) where T : IReconcileData, new()
        {
            if (_networkObjectCache.PredictionManager.IsReconciling)
                return;

            if (_networkObjectCache.IsServerInitialized)
                Reconcile_Server(hash, ref lastReconcileData, data, channel);
            else
                Reconcile_Client_AddToLocalHistory(reconcilesHistory, data);
        }

        /// <summary>
        /// Runs a reconcile. Prefers server data if available, otherwise uses local history data.
        /// </summary>
        [MakePublic]
        internal void Reconcile_Client<T, T2>(ReconcileUserLogicDelegate<T> reconcileDel, RingBuffer<ReplicateDataContainer<T2>> replicatesHistory, RingBuffer<LocalReconcile<T>> reconcilesHistory, T data) where T : IReconcileData where T2 : IReplicateData, new()
        {
            const long unsetHistoryIndex = -1;
            long historyIndex = unsetHistoryIndex;

            /* There should always be entries, except when the object
             * first spawns.
             *
             * Find the history index associated with the reconcile tick. */
            if (reconcilesHistory.Count > 0)
            {
                uint tickToFind = PredictionManager.ClientStateTick;

                //If reconcile data received then use that tick, otherwise get estimated tick for this reconcile.
                uint reconcileTick = tickToFind; // isBehaviourReconciling ? data.GetTick() : _networkObjectCache.PredictionManager.GetReconcileStateTick(_networkObjectCache.IsOwner);

                uint firstHistoryTick = reconcilesHistory[0].Tick;
                historyIndex = (long)reconcileTick - (long)firstHistoryTick;

                /* If difference is negative then
                 * the first history is beyond the tick being reconciled.
                 * EG: if history index 0 is 100 and reconcile tick is 90 then
                 * (90 - 100) = -10.
                 * This should only happen when first connecting and data hasn't been made yet. */
                if (!IsHistoryIndexValid((int)historyIndex))
                {
                    historyIndex = unsetHistoryIndex;
                    ClearReconcileHistory(reconcilesHistory, reconcileTick);
                }
                //Valid history index.
                else
                {
                    uint lrTick = reconcilesHistory[(int)historyIndex].Tick;
                    if (lrTick != reconcileTick)
                        historyIndex = unsetHistoryIndex;

                    //If index is set and behaviour is not reconciling then apply data.
                    if (!IsBehaviourReconciling && historyIndex != unsetHistoryIndex)
                    {
                        LocalReconcile<T> localReconcile = reconcilesHistory[(int)historyIndex];
                        //Before disposing get the writer and call reconcile reader so it's parsed.
                        PooledWriter reconcileWritten = localReconcile.Writer;
                        /* Although this is actually from the local client the datasource is being set to server since server
                         * is what typically sends reconciles. */
                        PooledReader reader = ReaderPool.Retrieve(reconcileWritten.GetArraySegment(), _networkObjectCache.NetworkManager, Reader.DataSource.Server);
                        data = Reconcile_Reader_Local<T>(localReconcile.Tick, reader);
                        ReaderPool.Store(reader);
                    }
                }
            }

            //Returns if a history index can be within history collection.
            bool IsHistoryIndexValid(int index) => index >= 0 && index < reconcilesHistory.Count;

            //Dispose of old reconcile histories.
            if (historyIndex != unsetHistoryIndex)
            {
                int index = (int)historyIndex;
                //If here everything is good, remove up to used index.
                for (int i = 0; i < index; i++)
                    reconcilesHistory[i].Dispose();

                reconcilesHistory.RemoveRange(true, (int)historyIndex);
            }

            //If this behaviour does not have data still then exit method.
            if (!IsBehaviourReconciling)
                return;

            uint dataTick = data.GetTick();
            _lastReconcileTick = dataTick;

            if (replicatesHistory.Count > 0)
            {
                /* Remove replicates up to reconcile. Since the reconcile
                 * is the state after a replicate for it's tick we no longer
                 * need any replicates prior. */
                //Find the closest entry which can be removed.
                int removeCount = 0;
                //A few quick tests.
                if (replicatesHistory.Count > 0)
                {
                    /* If the last entry in history is less or equal
                     * to datatick then all histories need to be removed
                     * as reconcile is beyond them. */
                    if (replicatesHistory[^1].Data.GetTick() <= dataTick)
                    {
                        removeCount = replicatesHistory.Count;
                    }
                    //Somewhere in between. Find what to remove up to.
                    else
                    {
                        for (int i = 0; i < replicatesHistory.Count; i++)
                        {
                            uint entryTick = replicatesHistory[i].Data.GetTick();
                            /* Soon as an entry beyond dataTick is
                             * found remove up to that entry. */
                            if (entryTick > dataTick)
                            {
                                removeCount = i;
                                break;
                            }
                        }
                    }
                }

                for (int i = 0; i < removeCount; i++)
                    replicatesHistory[i].Dispose();
                replicatesHistory.RemoveRange(true, removeCount);
            }


            //Set on the networkObject that a reconcile can now occur.
            _networkObjectCache.IsObjectReconciling = true;

            //Call reconcile user logic.
            reconcileDel?.Invoke(data, Channel.Reliable);
        }

        internal void Reconcile_Client_End()
        {
            IsBehaviourReconciling = false;
        }

        /// <summary>
        /// Disposes and clears LocalReconciles.
        /// </summary>
        /// <param name="stopTick">Tick when encountered removals will stop.</param>
        private void ClearReconcileHistory<T>(RingBuffer<LocalReconcile<T>> reconcilesHistory, uint stopTick) where T : IReconcileData
        {
            int removalCount = 0;
            foreach (LocalReconcile<T> localReconcile in reconcilesHistory)
            {
                if (localReconcile.Tick >= stopTick)
                    break;

                removalCount++;
                localReconcile.Dispose();
            }

            if (removalCount > 0)
                reconcilesHistory.RemoveRange(fromStart: true, removalCount);
        }

        /// <summary>
        /// Reads a reconcile from the server.
        /// </summary>
        public void Reconcile_Reader_Remote<T>(PooledReader reader, ref T lastReconcileData) where T : IReconcileData
        {
            uint tick = IsOwner ? PredictionManager.ClientStateTick : PredictionManager.ServerStateTick;
            #if DO_NOT_USE
            T newData = reader.ReadDeltaReconcile(lastReconciledata);
            #else
            T newData = reader.ReadReconcile<T>();
            #endif
            //Do not process if an old state.
            if (tick < _lastReadReconcileRemoteTick)
                return;

            lastReconcileData = newData;
            lastReconcileData.SetTick(tick);

            /* This does not need to log to statistics -- network traffic
             * will be read when the state executes. */

            IsBehaviourReconciling = true;
            IsReconcileRemote = true;

            _networkObjectCache.IsObjectReconciling = true;
            _lastReadReconcileRemoteTick = tick;
        }

        /// <summary>
        /// Reads a local reconcile from the client.
        /// </summary>
        public T Reconcile_Reader_Local<T>(uint tick, PooledReader reader) where T : IReconcileData
        {
            reader.NetworkManager = _networkObjectCache.NetworkManager;
            T newData = reader.ReadReconcile<T>();
            newData.SetTick(tick);

            IsBehaviourReconciling = true;
            IsReconcileRemote = false;

            return newData;
        }

        /// <summary>
        /// Sets the last tick this NetworkBehaviour replicated with.
        /// </summary>
        /// <param name = "setUnordered">True to set unordered value, false to set ordered.</param>
        private void SetReplicateTick(uint value, bool createdReplicate)
        {
            _lastOrderedReplicatedTick = value;
            _networkObjectCache.SetReplicateTick(value, createdReplicate);
        }
    }
}