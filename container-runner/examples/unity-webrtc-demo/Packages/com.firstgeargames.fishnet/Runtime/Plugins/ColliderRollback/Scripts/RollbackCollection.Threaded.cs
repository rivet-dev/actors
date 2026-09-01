#if FISHNET_THREADED_COLLIDER_ROLLBACK
using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using FishNet.Managing;
using Unity.Collections;
using Unity.Jobs;
using Unity.Jobs.LowLevel.Unsafe;
using UnityEngine;
using UnityEngine.Jobs;

namespace FishNet.Component.ColliderRollback
{
	/// <summary>
	/// Holds all job-friendly data for collider rollback: flattened transform table,
	/// ring buffers of TRS snapshots, and a persistent rolled-back mask.
	/// </summary>
	public sealed partial class RollbackCollection
	{
		#region Private.
		/// <summary>
		/// NetworkManager.
		/// </summary>
		private NetworkManager _networkManager;
		/// <summary>
		/// True if the collection is configured and has valid buffers.
		/// </summary>
		private bool _ready;
		/// <summary>
		/// Ring buffer length per rolling entry.
		/// </summary>
		private int _maxSnapshots;
		/// <summary>
		/// Physics used when rolling back.
		/// </summary>
		private RollbackPhysicsType _rollbackPhysics;
		/// <summary>
		/// List of ColliderRollback.
		/// </summary>
		private readonly List<ColliderRollback> _colliderRollbacks = new();
		/// <summary>
		/// Map of ColliderRollback -> index.
		/// </summary>
		private readonly Dictionary<ColliderRollback, int> _colliderRollbacksIndices = new();
		/// <summary>
		/// Write array for Rollback requests for deferred Rollback.
		/// </summary>
		private NativeList<RollbackManager.RollbackRequest> _writeRequests;
		/// <summary>
		/// Read array for Rollback requests for deferred Rollback.
		/// </summary>
		private NativeList<RollbackManager.RollbackRequest> _readRequests;
		/// <summary>
		/// Physics that requested for deferred rollback.
		/// </summary>
		private RollbackPhysicsType _requestsRollbackPhysics;
		/// <summary>
		/// TransformAccessArray for ColliderRollbacks.
		/// </summary>
		private TransformAccessArray _colliderRollbacksTAA;
		/// <summary>
		/// TransformAccessArray for RollingColliders.
		/// </summary>
		private TransformAccessArray _rollingCollidersTAA;
		/// <summary>
		/// Snapshots of ColliderRollbacks: [colliderRollbackIndex].
		/// </summary>
		private NativeList<ColliderSnapshot> _colliderRollbacksSnapshots;
		/// <summary>
		/// Flattened snapshots ring-buffer of RollingColliders: [rollingColliderIndex * MaxSnapshots + frame].
		/// </summary>
		private NativeList<ColliderSnapshot> _rollingCollidersSnapshots;
		/// <summary>
		/// ColliderRollbacks-level rolled-back flags (0 = write, 1 = freeze).
		/// </summary>
		private NativeList<byte> _colliderRollbacksRolledBackMask;
		/// <summary>
		/// Per-colliderRollbacks scene handle to allow filtering in jobs without masks.
		/// </summary>
		private NativeList<int> _colliderRollbacksSceneHandles;
		/// <summary>
		/// Per-colliderRollbacks number of available history frames for lerp (clamped to MaxSnapshots).
		/// </summary>
		private NativeList<int> _colliderRollbacksLerpFrames;
		/// <summary>
		/// Per-colliderRollbacks BoundingBoxData.
		/// </summary>
		private NativeList<RollbackManager.BoundingBoxData> _colliderRollbacksBoundingBoxData;
		/// <summary>
		/// Maps rolling-collider index to its colliderRollbacks index.
		/// </summary>
		private NativeList<int> _rollingColliderToColliderRollbacks;
		/// <summary>x
		/// Per-rolling write pointer (next frame slot in the ring).
		/// </summary>
		private NativeList<int> _rollingCollidersWriteIndices;
		#endregion

		~RollbackCollection() { Deinitialize(); }
		/// <summary>
		/// Initialize ring size based on network timing. Call once on startup or when timing changes.
		/// </summary>
		internal void Initialize(NetworkManager networkManager, double tickDelta, float maximumRollbackTime)
		{
			_networkManager = networkManager;
			_maxSnapshots = Mathf.Max(2, Mathf.CeilToInt((float)(maximumRollbackTime / tickDelta)));
			if (!_writeRequests.IsCreated) _writeRequests = new NativeList<RollbackManager.RollbackRequest>(64, Allocator.Persistent);
			if (!_readRequests.IsCreated) _readRequests = new NativeList<RollbackManager.RollbackRequest>(64, Allocator.Persistent);
			if (!_colliderRollbacksTAA.isCreated) _colliderRollbacksTAA = new TransformAccessArray(64);
			if (!_rollingCollidersTAA.isCreated) _rollingCollidersTAA = new TransformAccessArray(64);
			if (!_colliderRollbacksSnapshots.IsCreated) _colliderRollbacksSnapshots = new NativeList<ColliderSnapshot>(64, Allocator.Persistent);
			if (!_rollingCollidersSnapshots.IsCreated) _rollingCollidersSnapshots = new NativeList<ColliderSnapshot>(64, Allocator.Persistent);
			if (!_colliderRollbacksRolledBackMask.IsCreated) _colliderRollbacksRolledBackMask = new NativeList<byte>(64, Allocator.Persistent);
			if (!_colliderRollbacksSceneHandles.IsCreated) _colliderRollbacksSceneHandles = new NativeList<int>(64, Allocator.Persistent);
			if (!_colliderRollbacksLerpFrames.IsCreated) _colliderRollbacksLerpFrames = new NativeList<int>(64, Allocator.Persistent);
			if (!_colliderRollbacksBoundingBoxData.IsCreated) _colliderRollbacksBoundingBoxData = new NativeList<RollbackManager.BoundingBoxData>(64, Allocator.Persistent);
			if (!_rollingColliderToColliderRollbacks.IsCreated) _rollingColliderToColliderRollbacks = new NativeList<int>(64, Allocator.Persistent);
			if (!_rollingCollidersWriteIndices.IsCreated) _rollingCollidersWriteIndices = new NativeList<int>(64, Allocator.Persistent);
			
			_ready = true;
		}
		/// <summary>
		/// Deinitialize all native buffers and transform access arrays.
		/// </summary>
		internal void Deinitialize()
		{
			_networkManager = null;
			if (_writeRequests.IsCreated) _writeRequests.Dispose();
			if (_readRequests.IsCreated) _readRequests.Dispose();
			if (_colliderRollbacksTAA.isCreated) _colliderRollbacksTAA.Dispose();
			if (_rollingCollidersTAA.isCreated) _rollingCollidersTAA.Dispose();
			if (_colliderRollbacksSnapshots.IsCreated) _colliderRollbacksSnapshots.Dispose();
			if (_rollingCollidersSnapshots.IsCreated) _rollingCollidersSnapshots.Dispose();
			if (_colliderRollbacksRolledBackMask.IsCreated) _colliderRollbacksRolledBackMask.Dispose();
			if (_colliderRollbacksSceneHandles.IsCreated) _colliderRollbacksSceneHandles.Dispose();
			if (_colliderRollbacksLerpFrames.IsCreated) _colliderRollbacksLerpFrames.Dispose();
			if (_colliderRollbacksBoundingBoxData.IsCreated) _colliderRollbacksBoundingBoxData.Dispose();
			if (_rollingColliderToColliderRollbacks.IsCreated) _rollingColliderToColliderRollbacks.Dispose();
			if (_rollingCollidersWriteIndices.IsCreated) _rollingCollidersWriteIndices.Dispose();
			
			_colliderRollbacks.Clear();
			_colliderRollbacksIndices.Clear();
			_ready = false;
		}
		/// <summary>
		/// Ensure capacities for upcoming additions without reallocations.
		/// </summary>
		private void EnsureCapacity(int addRollingColliders, int addColliderRollbacks)
		{
			if (!_ready)
			{
				_networkManager.LogError("RollbackCollection is not configured. Call Configure(NetworkManager, double, float) first.");
				return;	
			}
			
			int newColliderRollbacksCount = _colliderRollbacksTAA.length + Math.Max(0, addColliderRollbacks);
			int newRollingCollidersCount = _rollingCollidersTAA.length + Math.Max(0, addRollingColliders);
			
			if (_colliderRollbacksTAA.capacity < newColliderRollbacksCount)
				_colliderRollbacksTAA.capacity = newColliderRollbacksCount;
			if (_rollingCollidersTAA.capacity < newRollingCollidersCount)
				_rollingCollidersTAA.capacity = newRollingCollidersCount;

			if (_colliderRollbacksSnapshots.Capacity < newColliderRollbacksCount)
				_colliderRollbacksSnapshots.Capacity = newColliderRollbacksCount;
			if (_rollingCollidersSnapshots.Capacity < newRollingCollidersCount * _maxSnapshots)
				_rollingCollidersSnapshots.Capacity = newRollingCollidersCount * _maxSnapshots;
			
			if (_colliderRollbacksRolledBackMask.Capacity < newColliderRollbacksCount)
				_colliderRollbacksRolledBackMask.Capacity = newColliderRollbacksCount;
			if (_colliderRollbacksSceneHandles.Capacity < newColliderRollbacksCount)
				_colliderRollbacksSceneHandles.Capacity = newColliderRollbacksCount;
			if (_colliderRollbacksLerpFrames.Capacity < newColliderRollbacksCount)
				_colliderRollbacksLerpFrames.Capacity = newColliderRollbacksCount;
			if (_colliderRollbacksBoundingBoxData.Capacity < newColliderRollbacksCount)
				_colliderRollbacksBoundingBoxData.Capacity = newColliderRollbacksCount;
			if (_rollingColliderToColliderRollbacks.Capacity < newRollingCollidersCount)
				_rollingColliderToColliderRollbacks.Capacity = newRollingCollidersCount;
			if (_rollingCollidersWriteIndices.Capacity < newRollingCollidersCount)
				_rollingCollidersWriteIndices.Capacity = newRollingCollidersCount;

		}
		/// <summary>
		/// Registers a ColliderRollback with all its RollingColliders.
		/// Adds new rolling entries at the end (dense indexing).
		/// </summary>
		internal void RegisterColliderRollback(ColliderRollback colliderRollback)
		{
			if (!_ready)
			{
				_networkManager.LogError("RollbackCollection is not configured. Call Configure(NetworkManager, double, float) first.");
				return;	
			}
			if (_colliderRollbacksIndices.ContainsKey(colliderRollback)) return;
			
			IReadOnlyList<Transform> list = colliderRollback.GetRollingColliders();
			int addColliders = list.Count;
			int newColliderRollbacksCount = _colliderRollbacks.Count + 1;
			int newRollingCollidersCount = _rollingCollidersTAA.length + addColliders;
			EnsureCapacity(addColliders, 1);
			
			_colliderRollbacks.Add(colliderRollback);
			_colliderRollbacksIndices[colliderRollback] = newColliderRollbacksCount - 1;
			_colliderRollbacksTAA.Add(colliderRollback.transform);
			_colliderRollbacksSnapshots.ResizeUninitialized(newColliderRollbacksCount);
			_colliderRollbacksRolledBackMask.Add(0);
			_colliderRollbacksSceneHandles.Add(colliderRollback.gameObject.scene.handle);
			_colliderRollbacksLerpFrames.Add(0);
			_colliderRollbacksBoundingBoxData.Add(colliderRollback.GetBoundingBoxData());

			_rollingCollidersSnapshots.ResizeUninitialized(newRollingCollidersCount * _maxSnapshots);
			for (int i = 0; i < addColliders; i++)
			{
				Transform rollingCollider = list[i];
				_rollingCollidersWriteIndices.Add(0);
				_rollingColliderToColliderRollbacks.Add(newColliderRollbacksCount - 1);
				_rollingCollidersTAA.Add(rollingCollider);
			}
		}
		/// <summary>
        /// Unregisters a ColliderRollback. Removes all its rolling entries.
        /// Uses swap-back removal for both rolling entries and the colliderRollbacks, keeping indices dense.
        /// </summary>
        internal void UnregisterColliderRollback(ColliderRollback cr)
        {
            if (!_ready) return;
            if (!_colliderRollbacksIndices.Remove(cr, out int colliderRollbacksIndex)) return;
            int lastColliderRollbacks = _colliderRollbacks.Count - 1;
            
            // Remove all rolling entries belonging to this colliderRollbacks (scan backwards for stability).
            for (int i = _rollingColliderToColliderRollbacks.Length - 1; i >= 0; --i)
            {
                if (_rollingColliderToColliderRollbacks[i] == colliderRollbacksIndex)
                    RemoveRollingColliderAtSwapBack(i);
            }

            // Remove the colliderRollbacks by swapping with the last colliderRollbacks.
            if (colliderRollbacksIndex != lastColliderRollbacks)
            {
	            ColliderRollback tempCr = _colliderRollbacks[lastColliderRollbacks];
	            _colliderRollbacksIndices[tempCr] = colliderRollbacksIndex;
	            _colliderRollbacks[colliderRollbacksIndex] = tempCr;
                // Re-tag colliders that belonged to lastColliderRollbacks to now point to colliderRollbacksIndex.
                for (int i = 0; i < _rollingColliderToColliderRollbacks.Length; i++)
                {
                    if (_rollingColliderToColliderRollbacks[i] == lastColliderRollbacks)
                        _rollingColliderToColliderRollbacks[i] = colliderRollbacksIndex;
                }
            }
            
            _colliderRollbacksTAA.RemoveAtSwapBack(colliderRollbacksIndex);
            _colliderRollbacksRolledBackMask.RemoveAtSwapBack(colliderRollbacksIndex);
            _colliderRollbacksSceneHandles.RemoveAtSwapBack(colliderRollbacksIndex);
            _colliderRollbacksLerpFrames.RemoveAtSwapBack(colliderRollbacksIndex);
            _colliderRollbacksBoundingBoxData.RemoveAtSwapBack(colliderRollbacksIndex);
            _colliderRollbacks.RemoveAt(lastColliderRollbacks);
        }
        /// <summary>
        /// Removes one rolling entry at index by swapping with the last item.
        /// Updates all parallel structures and GlobalIndex on the moved entry.
        /// </summary>
        internal void RemoveRollingColliderAtSwapBack(int rollingColliderIndex)
        {
            int last = _rollingCollidersTAA.length - 1;
            if (last < 0) return;
            
            _rollingCollidersTAA.RemoveAtSwapBack(rollingColliderIndex);
            _rollingCollidersWriteIndices.RemoveAtSwapBack(rollingColliderIndex);
            _rollingColliderToColliderRollbacks.RemoveAtSwapBack(rollingColliderIndex);
            if (rollingColliderIndex != last)
            {
                // Move last snapshots ring over the removed slot.
                int dst = rollingColliderIndex * _maxSnapshots;
                int src = last * _maxSnapshots;
                for (int k = 0; k < _maxSnapshots; k++)
                    _rollingCollidersSnapshots[dst + k] =_rollingCollidersSnapshots[src + k];
            }
            _rollingCollidersSnapshots.ResizeUninitialized(_rollingCollidersSnapshots.Length - _maxSnapshots);
        }
		/// <summary>
		/// Populates one snapshot for every non-rolled-back transform using a parallel job.
		/// Call this once per tick on the server (e.g., from OnPostTick).
		/// </summary>
		internal void CreateSnapshots()
		{
			if (!_ready) return;

			if (_colliderRollbacksTAA.length > 0 && _rollingCollidersTAA.length > 0)
			{
				JobHandle first = new RollbackManager.IncrementGroupsFramesJob
				{
					maxSnapshots = _maxSnapshots,
					colliderRollbacksLerpFrames = _colliderRollbacksLerpFrames.AsArray(),
					colliderRollbacksRolledBackMask = _colliderRollbacksRolledBackMask.AsArray(),
				}.Schedule(_colliderRollbacksTAA.length, 64);
				JobHandle second = new RollbackManager.PopulateColliderRollbackSnapshotsJob
				{
					colliderRollbackSnapshots = _colliderRollbacksSnapshots.AsArray(),
					colliderRollbacksRolledBackMask = _colliderRollbacksRolledBackMask.AsArray()
				}.Schedule(_colliderRollbacksTAA, first);
				JobHandle third = new RollbackManager.PopulateRollingColliderSnapshotsJob
				{
					maxSnapshots = _maxSnapshots,
					rollingCollidersSnapshots = _rollingCollidersSnapshots.AsArray(),
					rollingCollidersWriteIndices = _rollingCollidersWriteIndices.AsArray(),
					colliderRollbacksRolledBackMask = _colliderRollbacksRolledBackMask.AsArray(),
					colliderToColliderRollbacks = _rollingColliderToColliderRollbacks.AsArray()
				}.Schedule(_rollingCollidersTAA, second);
				third.Complete();
			}
		}

		#region Sinlge ColliderRollback 
		/// <summary>
		/// Computes lerp mode/endFrame/percent based on 'time' and applies rollback to the whole colliderRollbacks.
		/// </summary>
		internal void Rollback(ColliderRollback colliderRollback, float time, RollbackPhysicsType rollbackPhysicsType)
		{
			if (!_ready) return;
			if (!_colliderRollbacksIndices.TryGetValue(colliderRollback, out int colliderRollbacksIndex)) return;

			// Already rolled back.
			if (IsRolledBack(colliderRollback))
			{
				_networkManager.LogWarning("Colliders are already rolled back. Returning colliders forward first.");
				Return(colliderRollback, rollbackPhysicsType);
			}
			
			int frames = _colliderRollbacksLerpFrames[colliderRollbacksIndex];
			if (frames == 0) return;

			/* If time were 0.3f and delta was 0.2f then the
			 * result would be 1.5f. This indicates to lerp between
			 * the first snapshot, and one after. */
			float decimalFrame = time / (float)_networkManager.TimeManager.TickDelta;

			RollbackManager.FrameRollbackTypes mode;
			int endFrame;
			float percent;

			/* Rollback is beyond written quantity.
			 * Set to use the last snapshot. */
			if (decimalFrame > frames)
			{
				mode = RollbackManager.FrameRollbackTypes.Exact;
				// Be sure to subtract 1 to get last entry in snapshots.
				endFrame = frames - 1;
				// Not needed for exact but must be set.
				percent = 1f;
			}
			else
			{
				percent  = decimalFrame % 1f;
				endFrame = Mathf.CeilToInt(decimalFrame);
				
				/* If the end frame is larger than or equal to 1
				 * then a lerp between two snapshots can occur. If
				 * equal to 1 then the lerp would occur between 0 and 1. */
				if (endFrame >= 1)
				{
					mode = RollbackManager.FrameRollbackTypes.LerpMiddle;
				}
				// Rolling back only 1 frame.
				else
				{
					endFrame = 0;
					mode = RollbackManager.FrameRollbackTypes.LerpFirst;
				}
			}

			// Apply to all rolling entries belonging to this colliderRollbacks.
			for (int i = 0; i < _rollingColliderToColliderRollbacks.Length; i++)
				if (_rollingColliderToColliderRollbacks[i] == colliderRollbacksIndex)
					ApplyRollbackIndex(i, endFrame, percent, mode);
			_colliderRollbacksRolledBackMask[colliderRollbacksIndex] = 1;
			
			_rollbackPhysics |= rollbackPhysicsType;
			SyncTransforms(rollbackPhysicsType);
		}
		/// <summary>
		/// Called when a specific colliderRollbacks should return.
		/// </summary>
		internal void Return(ColliderRollback colliderRollback, RollbackPhysicsType rollbackPhysicsType)
		{
			if (!_ready) return;
			if (!_colliderRollbacksIndices.TryGetValue(colliderRollback, out int colliderRollbacksIndex)) return;
			
			if (!IsRolledBack(colliderRollback))
				return;
			
			// Iterate dense rolling entries and return only those that belong to this colliderRollbacks.
			for (int i = 0; i < _rollingColliderToColliderRollbacks.Length; i++)
			{
				if (_rollingColliderToColliderRollbacks[i] == colliderRollbacksIndex)
				{
					int frames = _colliderRollbacksLerpFrames[colliderRollbacksIndex];
					if (frames <= 0)
						continue;

					int writeIdx = _rollingCollidersWriteIndices[i];
					int baseOffset = i * _maxSnapshots;
					int lastIdx = (writeIdx - 1 + _maxSnapshots) % _maxSnapshots;

					// Return to the newest (last written) snapshot
					int snapshotIndex = baseOffset + lastIdx;
					ColliderSnapshot s = _rollingCollidersSnapshots[snapshotIndex];
					Transform t = _rollingCollidersTAA[i];
					t.SetPositionAndRotation(s.WorldPosition, s.WorldRotation);
				}
			}
			_colliderRollbacksRolledBackMask[colliderRollbacksIndex] = 0;
			
			_rollbackPhysics |= rollbackPhysicsType;
			SyncTransforms(rollbackPhysicsType);
		}
		/// <summary>
		/// Applies a rollback for a specific global transform index using the provided interpolation mode.
		/// </summary>
		/// <param name="rollingColliderIdx">RollingCollider index into the ColliderRollback.</param>
		/// <param name="mode">Frame interpolation mode.</param>
		/// <param name="endFrame">Target history frame index (0 = newest).</param>
		/// <param name="percent">Lerp factor for interpolation modes.</param>
		[MethodImpl(MethodImplOptions.AggressiveInlining)]
		private void ApplyRollbackIndex(int rollingColliderIdx, int endFrame, float percent,
			RollbackManager.FrameRollbackTypes mode)
		{
			if (!_ready || rollingColliderIdx < 0 || rollingColliderIdx >= _rollingCollidersTAA.length)
				return;

			int colliderRollbacksIndex = _rollingColliderToColliderRollbacks[rollingColliderIdx];
			int writeIdx = _rollingCollidersWriteIndices[rollingColliderIdx];
			int baseOffset = rollingColliderIdx * _maxSnapshots;
			int lastIdx = (writeIdx - 1 + _maxSnapshots) % _maxSnapshots;
			bool isRecycled = _colliderRollbacksLerpFrames[colliderRollbacksIndex] >= _maxSnapshots;

			Transform t = _rollingCollidersTAA[rollingColliderIdx];

			if (mode == RollbackManager.FrameRollbackTypes.Exact)
			{
				ColliderSnapshot s = _rollingCollidersSnapshots[BufIndex(baseOffset, endFrame, lastIdx, isRecycled, _maxSnapshots)];
				t.SetPositionAndRotation(s.WorldPosition, s.WorldRotation);
			}
			else if (mode == RollbackManager.FrameRollbackTypes.LerpFirst)
			{
				ColliderSnapshot s = _rollingCollidersSnapshots[BufIndex(baseOffset, endFrame, lastIdx, isRecycled, _maxSnapshots)];
				t.GetPositionAndRotation(out Vector3 curPos, out Quaternion curRot);
				t.SetPositionAndRotation(Vector3.Lerp(curPos, s.WorldPosition, percent),
					Quaternion.Lerp(curRot, s.WorldRotation, percent));
			}
			else if (mode == RollbackManager.FrameRollbackTypes.LerpMiddle)
			{
				ColliderSnapshot s0 = _rollingCollidersSnapshots[BufIndex(baseOffset, endFrame - 1, lastIdx, isRecycled, _maxSnapshots)];
				ColliderSnapshot s1 = _rollingCollidersSnapshots[BufIndex(baseOffset, endFrame, lastIdx, isRecycled, _maxSnapshots)];
				t.SetPositionAndRotation(Vector3.Lerp(s0.WorldPosition, s1.WorldPosition, percent),
					Quaternion.Lerp(s0.WorldRotation, s1.WorldRotation, percent));
			}
			return;

			// compute buffer index with negative-safe modulo
			[MethodImpl(MethodImplOptions.AggressiveInlining)]
			static int BufIndex(int baseOffset, int history, int lastIdx, bool isRecycled, int maxSnapshots)
			{
				int idx = baseOffset + lastIdx - history;
				// If negative value start taking from the back.
				if (idx < 0)
				{
					/* Cannot take from back, snapshots aren't filled yet.
					 * Instead take the oldest snapshot, which in this case
					 * would be index baseOffset. */
					if (!isRecycled)
						return baseOffset;
					// Snapshots filled, take from back.
					else
						return idx + maxSnapshots;
				}
				// Not a negative value, return as is.
				else return idx;
			}
		}
		/// <summary>
		/// Sets rolled-back state for a colliderRollbacks in O(1). No per-rolling writes.
		/// </summary>
		/// <summary>True if the colliderRollbacks is currently rolled back.</summary>
		public bool IsRolledBack(ColliderRollback cr)
			=> _ready && _colliderRollbacksIndices.TryGetValue(cr, out int g) && _colliderRollbacksRolledBackMask[g] != 0;
		#endregion

		#region Job ColliderRollback
		/// <summary>
		/// Run rollback for ALL colliderRollbacks in parallel (jobified).
		/// </summary>
		internal void Rollback(int sceneHandle, float time, RollbackPhysicsType rollbackPhysicsType)
		{
			if (!_ready || _rollingCollidersTAA.length == 0)
				return;

			Return();
			JobHandle job = new RollbackManager.ApplyRollbackJob
			{
				sceneHandle       = sceneHandle,
				maxSnapshots      = _maxSnapshots,
				decimalFrame      = time / (float)_networkManager.TimeManager.TickDelta,
				colliderToColliderRollbacks      = _rollingColliderToColliderRollbacks.AsArray(),
				colliderRollbacksSceneHandles    = _colliderRollbacksSceneHandles.AsArray(),
				colliderRollbacksLerpFrames      = _colliderRollbacksLerpFrames.AsArray(),
				rollingCollidersWriteIndices     = _rollingCollidersWriteIndices.AsArray(),
				colliderRollbacksRolledBackMask  = _colliderRollbacksRolledBackMask.AsArray(),
				rollingCollidersSnapshots        = _rollingCollidersSnapshots.AsArray()
			}.Schedule(_rollingCollidersTAA);
			job.Complete();
			
			_rollbackPhysics |= rollbackPhysicsType;
			SyncTransforms(rollbackPhysicsType);
		}
		/// <summary>
		/// Run rollback for intersected colliderRollbacks by ray in parallel (jobified).
		/// </summary>
		internal void Rollback(RollbackManager.RollbackRequest rollbackRequest)
		{
			if (!_ready || _rollingCollidersTAA.length == 0)
				return;

			Return();
			JobHandle job = new RollbackManager.ApplyRollbackRaycastJob
			{
				sceneHandle  = rollbackRequest.sceneHandle,
				maxSnapshots = _maxSnapshots,
				decimalFrame = rollbackRequest.time / (float)_networkManager.TimeManager.TickDelta,
				origin       = rollbackRequest.origin,
				dir          = rollbackRequest.direction,
				distance     = rollbackRequest.distance,
				physicsType  = (int)rollbackRequest.rollbackPhysicsType,
				colliderToColliderRollbacks      = _rollingColliderToColliderRollbacks.AsArray(),
				colliderRollbacksBoundingBoxData = _colliderRollbacksBoundingBoxData.AsArray(),
				colliderRollbacksSceneHandles    = _colliderRollbacksSceneHandles.AsArray(),
				colliderRollbacksLerpFrames      = _colliderRollbacksLerpFrames.AsArray(),
				rollingCollidersWriteIndices     = _rollingCollidersWriteIndices.AsArray(),
				colliderRollbacksRolledBackMask  = _colliderRollbacksRolledBackMask.AsArray(),
				colliderRollbacksSnapshots       = _colliderRollbacksSnapshots.AsArray(),
				rollingCollidersSnapshots        = _rollingCollidersSnapshots.AsArray()
			}.Schedule(_rollingCollidersTAA);
			job.Complete();
			
			_rollbackPhysics |= rollbackRequest.rollbackPhysicsType;
			SyncTransforms(rollbackRequest.rollbackPhysicsType);
		}
		/// <summary>
		/// Request rollback for deferred rollback for intersected colliderRollbacks by ray in parallel (jobified).
		/// </summary>
		internal void RequestRollbackDeferred(RollbackManager.RollbackRequest rollbackRequest)
		{
			_writeRequests.Add(rollbackRequest);
			_requestsRollbackPhysics |= rollbackRequest.rollbackPhysicsType;
		}
		
		/// <summary>
		/// Run rollback for all requests intersected colliderRollbacks by ray in parallel (jobified).
		/// </summary>
		/// <returns>Count of requests.</returns>
		internal int RollbackDeferred()
		{
			if (!_ready || _rollingCollidersTAA.length == 0 || _writeRequests.Length == 0)
				return 0;

			Return();
			
			(_readRequests, _writeRequests) = (_writeRequests, _readRequests);
			_writeRequests.Clear();
			
			int groupCount = _colliderRollbacksTAA.length;
			int batchSize = ComputeBatchSize(groupCount);
			
			NativeArray<float> sum = new NativeArray<float>(groupCount, Allocator.TempJob);
			NativeArray<int> cnt = new NativeArray<int>(groupCount, Allocator.TempJob);
			try
			{
				JobHandle computeHandle = new RollbackManager.ComputeDeferredRollbackSumsJob
				{
					tickDelta = (float)_networkManager.TimeManager.TickDelta,
					requests = _readRequests.AsArray(),
					colliderRollbacksSceneHandles = _colliderRollbacksSceneHandles.AsArray(),
					colliderRollbacksLerpFrames = _colliderRollbacksLerpFrames.AsArray(),
					colliderRollbacksBoundingBoxData = _colliderRollbacksBoundingBoxData.AsArray(),
					colliderRollbacksSnapshots = _colliderRollbacksSnapshots.AsArray(),
					sumDecimalFrame = sum,
					hitCount = cnt
				}.Schedule(groupCount, batchSize);

				JobHandle applyHandle = new RollbackManager.ApplyDeferredRollbackJob
				{
					maxSnapshots = _maxSnapshots,
					colliderToColliderRollbacks = _rollingColliderToColliderRollbacks.AsArray(),
					colliderRollbacksLerpFrames = _colliderRollbacksLerpFrames.AsArray(),
					rollingCollidersWriteIndices = _rollingCollidersWriteIndices.AsArray(),
					colliderRollbacksRolledBackMask = _colliderRollbacksRolledBackMask.AsArray(),
					rollingCollidersSnapshots = _rollingCollidersSnapshots.AsArray(),
					sumDecimalFrame = sum,
					hitCount = cnt
				}.Schedule(_rollingCollidersTAA, computeHandle);

				applyHandle.Complete();
			}
			finally
			{
				sum.Dispose();
				cnt.Dispose();
			}

			_rollbackPhysics |= _requestsRollbackPhysics;
			_requestsRollbackPhysics = 0;
			SyncTransforms(_rollbackPhysics);
			return _readRequests.Length;
		}

		/// <summary>
		/// Run Return for ALL colliderRollbacks in parallel (jobified).
		/// </summary>
		internal void Return()
		{
			if (!_ready || _rollingCollidersTAA.length == 0)
				return;
			
			JobHandle job = new RollbackManager.ReturnRollbackAllJob
			{
				maxSnapshots                    = _maxSnapshots,
				colliderToColliderRollbacks     = _rollingColliderToColliderRollbacks.AsArray(),
				colliderRollbacksLerpFrames     = _colliderRollbacksLerpFrames.AsArray(),
				rollingCollidersWriteIndices    = _rollingCollidersWriteIndices.AsArray(),
				colliderRollbacksRolledBackMask = _colliderRollbacksRolledBackMask.AsArray(),
				rollingCollidersSnapshots       = _rollingCollidersSnapshots.AsArray()
			}.Schedule(_rollingCollidersTAA);
			job.Complete();
			
			SyncTransforms(_rollbackPhysics);
		}
		#endregion
		
		/// <summary>
		/// Applies transforms for the specified physics type.
		/// </summary>
		/// <param name = "physicsType"></param>
		private static void SyncTransforms(RollbackPhysicsType physicsType)
		{
			if ((physicsType & RollbackPhysicsType.Physics) > 0)
				Physics.SyncTransforms();
		    if ((physicsType & RollbackPhysicsType.Physics2D) > 0)
				Physics2D.SyncTransforms();
		}
		
		private static int ComputeBatchSize(int length, int minBatch = 1, int maxBatch = 128)
		{
			if (length <= 0) return 1;

			// +1: main thread + worker threads
			int workers = JobsUtility.JobWorkerCount + 1;

			// Aim for ~4 waves of batches across all workers.
			int targetBatches = Mathf.Max(1, workers * 4);

			// CeilDiv to get iterations per batch
			int batch = (length + targetBatches - 1) / targetBatches;

			return Mathf.Clamp(batch, minBatch, maxBatch);
		}
	}
}
#endif