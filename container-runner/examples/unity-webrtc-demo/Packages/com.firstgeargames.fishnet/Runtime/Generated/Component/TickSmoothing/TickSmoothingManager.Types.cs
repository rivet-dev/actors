#if FISHNET_THREADED_TICKSMOOTHERS
using System;
using System.Runtime.CompilerServices;
using FishNet.Managing.Timing;
using FishNet.Object;
using FishNet.Object.Prediction;
using FishNet.Utility.Extension;
using GameKit.Dependencies.Utilities.Types;
using Unity.Burst;
using Unity.Collections;
using Unity.Jobs;
using Unity.Mathematics;
using UnityEngine;
using UnityEngine.Jobs;
using UnityEngine.Scripting;

namespace FishNet.Component.Transforming.Beta
{
	public partial class TickSmoothingManager
	{
		#region Types.
		[Preserve]
		public struct TickTransformProperties
		{
			public readonly uint Tick;
			public readonly TransformProperties Properties;

			public TickTransformProperties(uint tick, TransformProperties properties)
			{
				Tick = tick;
				Properties = properties;
			}
		}
		[Preserve]
		private struct NullableTransformProperties
		{
			public readonly byte IsExist;
			public readonly TransformProperties Properties;

			public NullableTransformProperties(bool isExist, TransformProperties properties)
			{
				IsExist = (byte)(isExist ? 1 : 0);
				Properties = properties;
			}
		}
		[Preserve]
		private struct MoveToTargetPayload
		{
			public byte executeMask;
			public float delta;

			public MoveToTargetPayload(byte executeMask, float delta)
			{
				this.executeMask = executeMask;
				this.delta = delta;
			}
		}
		[Preserve]
		private struct UpdateRealtimeInterpolationPayload
		{
			public byte executeMask;

			public UpdateRealtimeInterpolationPayload(byte executeMask)
			{
				this.executeMask = executeMask;
			}
		}
		[Preserve]
		private struct DiscardExcessiveTransformPropertiesQueuePayload
		{
			public byte executeMask;

			public DiscardExcessiveTransformPropertiesQueuePayload(byte executeMask)
			{
				this.executeMask = executeMask;
			}
		}
		[Preserve]
		private struct SetMoveRatesPayload
		{
			public byte executeMask;
			public TransformProperties prevValues;
			
			public SetMoveRatesPayload(byte executeMask, TransformProperties prevValues)
			{
				this.executeMask = executeMask;
				this.prevValues = prevValues;
			}
		}
		[Preserve]
		private struct SetMovementMultiplierPayload
		{
			public byte executeMask;
			
			public SetMovementMultiplierPayload(byte executeMask)
			{
				this.executeMask = executeMask;
			}
		}
		[Preserve]
		private struct AddTransformPropertiesPayload
		{
			public byte executeMask;
			public TickTransformProperties tickTransformProperties;
			
			public AddTransformPropertiesPayload(byte executeMask, TickTransformProperties tickTransformProperties)
			{
				this.executeMask = executeMask;
				this.tickTransformProperties = tickTransformProperties;
			}
		}
		[Preserve]
		private struct ClearTransformPropertiesQueuePayload
		{
			public byte executeMask;
			
			public ClearTransformPropertiesQueuePayload(byte executeMask)
			{
				this.executeMask = executeMask;
			}
		}
		[Preserve]
		private struct ModifyTransformPropertiesPayload
		{
			public byte executeMask;
			public uint clientTick;
			public uint firstTick;
			
			public ModifyTransformPropertiesPayload(byte executeMask, uint clientTick, uint firstTick)
			{
				this.executeMask = executeMask;
				this.clientTick = clientTick;
				this.firstTick = firstTick;
			}
		}
		[Preserve]
		private struct SnapNonSmoothedPropertiesPayload
		{
			public byte executeMask;
			public TransformProperties goalValues;
			
			public SnapNonSmoothedPropertiesPayload(byte executeMask, TransformProperties goalValues)
			{
				this.executeMask = executeMask;
				this.goalValues = goalValues;
			}
		}
		[Preserve]
		private struct TeleportPayload
		{
			public byte executeMask;
			
			public TeleportPayload(byte executeMask)
			{
				this.executeMask = executeMask;
			}
		}
		#endregion
		
		#region PreTick.
		[BurstCompile]
		private struct PreTickMarkJob : IJobParallelFor
		{
			[ReadOnly] public NativeArray<byte> canSmoothMask;
			[WriteOnly] public NativeArray<byte> preTickedMask;
			
			[WriteOnly] public NativeArray<DiscardExcessiveTransformPropertiesQueuePayload> discardExcessivePayloads;

			public void Execute(int index)
			{
				discardExcessivePayloads[index] = new DiscardExcessiveTransformPropertiesQueuePayload(0);
				
				if (canSmoothMask[index] == 0)
					return;
				
				preTickedMask[index] = 1;
				discardExcessivePayloads[index] = new DiscardExcessiveTransformPropertiesQueuePayload(1);
			}
		}
		
		[BurstCompile]
		private struct PreTickCaptureGraphicalJob : IJobParallelForTransform
		{
			[ReadOnly] public NativeArray<byte> canSmoothMask;
			[ReadOnly] public NativeArray<byte> useOwnerSettingsMask;
			[ReadOnly] public NativeArray<MovementSettings> ownerSettings;
			[ReadOnly] public NativeArray<MovementSettings> spectatorSettings;
			[WriteOnly] public NativeArray<TransformProperties> graphicSnapshot;

			public void Execute(int index, TransformAccess graphicalTransform)
			{
				if (canSmoothMask[index] == 0)
					return;
				
				byte isOwner = useOwnerSettingsMask[index];
				MovementSettings settings = isOwner != 0 ? ownerSettings[index] : spectatorSettings[index];
				
				bool useLocalSpace = settings.UseLocalSpace;
				graphicSnapshot[index] = GetTransformProperties(graphicalTransform, useLocalSpace);
			}
		}
		#endregion
		
		#region PostTick
		
		[BurstCompile]
		private struct PostTickCaptureTrackerJob : IJobParallelForTransform
		{
			[ReadOnly] public NativeArray<byte> canSmoothMask;
			[ReadOnly] public NativeArray<byte> detachOnStartMask;
			[ReadOnly] public NativeArray<byte> useOwnerSettingsMask;
			[ReadOnly] public NativeArray<MovementSettings> ownerSettings;
			[ReadOnly] public NativeArray<MovementSettings> spectatorSettings;
			[ReadOnly] public NativeArray<TransformProperties> targetSnapshot;
			[WriteOnly] public NativeArray<TransformProperties> trackerSnapshot;
			
			public void Execute(int index, TransformAccess trackerTransform)
			{
				if (canSmoothMask[index] == 0)
					return;

				bool isDetach = detachOnStartMask[index] != 0;
				byte isOwner = useOwnerSettingsMask[index];
				MovementSettings settings = isOwner != 0 ? ownerSettings[index] : spectatorSettings[index];
				
				bool useLocalSpace = settings.UseLocalSpace;
				TransformProperties trackerProperties = GetTrackerTransformProperties(trackerTransform, isDetach, useLocalSpace);
				if (useLocalSpace) trackerProperties += targetSnapshot[index];
				trackerSnapshot[index] = trackerProperties;
			}
		}
		[BurstCompile]
		private struct PostTickJob : IJobParallelForTransform
		{
			[ReadOnly] public uint clientTick;
			[ReadOnly] public NativeArray<byte> canSmoothMask;
			[ReadOnly] public NativeArray<uint> teleportedTick;
			[ReadOnly] public NativeArray<byte> preTickedMask;
			[ReadOnly] public NativeArray<byte> detachOnStartMask;
			[ReadOnly] public NativeArray<TransformProperties> postTickTrackerSnapshot;
			[ReadOnly] public NativeArray<TransformProperties> preTickGraphicSnapshot;
			[ReadOnly] public NativeArray<byte> useOwnerSettingsMask;
			[ReadOnly] public NativeArray<MovementSettings> ownerSettings;
			[ReadOnly] public NativeArray<MovementSettings> spectatorSettings;
			
			[WriteOnly] public NativeArray<DiscardExcessiveTransformPropertiesQueuePayload> discardExcessivePayloads;
			[WriteOnly] public NativeArray<SnapNonSmoothedPropertiesPayload> snapNonSmoothedPropertiesPayloads;
			[WriteOnly] public NativeArray<AddTransformPropertiesPayload> addTransformPropertiesPayloads;
			
			public void Execute(int index, TransformAccess graphicalTransform)
			{
				discardExcessivePayloads[index] = new DiscardExcessiveTransformPropertiesQueuePayload(0);
				addTransformPropertiesPayloads[index] = new AddTransformPropertiesPayload(0, default);
				
				if (canSmoothMask[index] == 0)
					return;

				if (clientTick <= teleportedTick[index])
					return;
				
				byte isOwner = useOwnerSettingsMask[index];
				MovementSettings settings = isOwner != 0 ? ownerSettings[index] : spectatorSettings[index];
				
				bool useLocalSpace = settings.UseLocalSpace;
				TransformProperties trackerProps = postTickTrackerSnapshot[index];
				//If preticked then previous transform values are known.
				if (preTickedMask[index] != 0)
				{
					//Only needs to be put to pretick position if not detached.
					if (detachOnStartMask[index] == 0)
					{
						var graphicProps = preTickGraphicSnapshot[index];
						SetTransformProperties(graphicalTransform, graphicProps, useLocalSpace);
					}

					TickTransformProperties tickTrackerProps = new TickTransformProperties(clientTick, trackerProps);
					discardExcessivePayloads[index] = new DiscardExcessiveTransformPropertiesQueuePayload(1);
					snapNonSmoothedPropertiesPayloads[index] = new SnapNonSmoothedPropertiesPayload(0, trackerProps);
					addTransformPropertiesPayloads[index] = new AddTransformPropertiesPayload(1, tickTrackerProps);
				}
				//If did not pretick then the only thing we can do is snap to instantiated values.
				else
				{
					//Only set to position if not to detach.
					if (detachOnStartMask[index] == 0)
					{
						SetTransformProperties(graphicalTransform, trackerProps, useLocalSpace);
					}
				}
			}
		}
		#endregion

		#region PostReplicateReplay
		[BurstCompile]
		private struct PostReplicateReplayJob : IJobParallelFor
		{
			[ReadOnly] public uint clientTick;
			[ReadOnly] public NativeArray<uint> teleportedTick;
			[ReadOnly] public NativeArray<byte> objectReconcilingMask;
			
			public StripedRingQueue<TickTransformProperties> transformProperties;
			[WriteOnly] public NativeArray<ModifyTransformPropertiesPayload> modifyTransformPropertiesPayloads;
			
			public void Execute(int index)
			{
				modifyTransformPropertiesPayloads[index] = new ModifyTransformPropertiesPayload(0, default, default);
				if (objectReconcilingMask[index] == 0)
					return;

				if (transformProperties.GetCount(index) == 0)
					return;
				if (clientTick <= teleportedTick[index])
					return;
				uint firstTick = transformProperties.Peek(index).Tick;
				//Already in motion to first entry, or first entry passed tick.
				if (clientTick <= firstTick)
					return;

				modifyTransformPropertiesPayloads[index] = new ModifyTransformPropertiesPayload(1, clientTick, firstTick);
			}
		}
		#endregion

		#region RoundTripTimeUpdated
		[BurstCompile]
		private struct RoundTripTimeUpdatedJob : IJobParallelFor
		{
			[ReadOnly] public NativeArray<byte> useOwnerSettingsMask;
			[ReadOnly] public NativeArray<MovementSettings> ownerSettings;
			[ReadOnly] public NativeArray<MovementSettings> spectatorSettings;
			
			[WriteOnly] public NativeArray<UpdateRealtimeInterpolationPayload> updateRealtimeInterpolationPayloads;
			
			public void Execute(int index)
			{
				updateRealtimeInterpolationPayloads[index] = new UpdateRealtimeInterpolationPayload(0);
				
				byte isOwner = useOwnerSettingsMask[index];
				MovementSettings settings = isOwner != 0 ? ownerSettings[index] : spectatorSettings[index];
				
				// Update RTT only if Adaptive Interpolation is not Off
				if (GetUseAdaptiveInterpolation(settings))
					updateRealtimeInterpolationPayloads[index] = new UpdateRealtimeInterpolationPayload(1);
			}
			
			private static bool GetUseAdaptiveInterpolation(in MovementSettings settings)
			{
				if (settings.AdaptiveInterpolationValue == AdaptiveInterpolationType.Off)
					return false;

				return true;
			}
		}
		#endregion

		#region Update
		[BurstCompile]
		private struct UpdateJob : IJobParallelFor
		{
			[ReadOnly] public NativeArray<byte> canSmoothMask;
			[ReadOnly] public float deltaTime;
			
			[WriteOnly] public NativeArray<MoveToTargetPayload> moveToTargetPayloads;
			
			public void Execute(int index)
			{
				moveToTargetPayloads[index] = new MoveToTargetPayload(0, default);
				
				if (canSmoothMask[index] == 0)
					return;
				
				moveToTargetPayloads[index] = new MoveToTargetPayload(1, deltaTime);
			}
		}
		#endregion

		#region Methods.
		[BurstCompile]
		private struct CaptureLocalTargetJob : IJobParallelForTransform
		{
			[ReadOnly] public NativeArray<byte> canSmoothMask;
			[ReadOnly] public NativeArray<byte> useOwnerSettingsMask;
			[ReadOnly] public NativeArray<MovementSettings> ownerSettings;
			[ReadOnly] public NativeArray<MovementSettings> spectatorSettings;
			[WriteOnly] public NativeArray<TransformProperties> targetSnapshot;
			
			public void Execute(int index, TransformAccess targetTransform)
			{
				if (canSmoothMask[index] == 0)
					return;
				
				byte isOwner = useOwnerSettingsMask[index];
				MovementSettings settings = isOwner != 0 ? ownerSettings[index] : spectatorSettings[index];
				
				bool useLocalSpace = settings.UseLocalSpace;
				if (!useLocalSpace) return;
				
				targetSnapshot[index] = GetTransformProperties(targetTransform, true);
			}
		}
		
		[BurstCompile]
		private struct MoveToTargetJob : IJobParallelForTransform
		{
			public NativeArray<MoveToTargetPayload> jobPayloads;
			
			[ReadOnly] public NativeArray<byte> useOwnerSettingsMask;
			[ReadOnly] public NativeArray<MovementSettings> ownerSettings;
			[ReadOnly] public NativeArray<MovementSettings> spectatorSettings;

			[ReadOnly] public NativeArray<byte> realTimeInterpolations;
			[ReadOnly] public NativeArray<byte> moveImmediatelyMask;
			[ReadOnly] public float tickDelta;
			
			public NativeArray<byte> isMoving;
			public NativeArray<float> movementMultipliers;
			
			public StripedRingQueue<TickTransformProperties> transformProperties;
			public NativeArray<MoveRates> moveRates;
			
			public NativeArray<SetMoveRatesPayload> setMoveRatesPayloads;
			public NativeArray<SetMovementMultiplierPayload> setMovementMultiplierPayloads;
			public NativeArray<ClearTransformPropertiesQueuePayload> clearTransformPropertiesQueuePayloads;
						
			public void Execute(int index, TransformAccess graphicalTransform)
			{
				MoveToTarget(
					index,
					graphicalTransform,
					ref jobPayloads,
					ref useOwnerSettingsMask,
					ref ownerSettings,
					ref spectatorSettings,
					ref realTimeInterpolations,
					ref moveImmediatelyMask,
					tickDelta,
					ref isMoving,
					ref movementMultipliers,
					ref transformProperties,
					ref moveRates,
					ref setMoveRatesPayloads,
					ref setMovementMultiplierPayloads,
					ref clearTransformPropertiesQueuePayloads);
			}
			
			/// <summary>
		    /// Moves transform to target values.
		    /// </summary>
		    public static void MoveToTarget(
		        int index,
		        TransformAccess graphicalTransform,
		        ref NativeArray<MoveToTargetPayload> jobPayloads,
		        ref NativeArray<byte> useOwnerSettingsMask,
		        ref NativeArray<MovementSettings> ownerSettings,
		        ref NativeArray<MovementSettings> spectatorSettings,
		        ref NativeArray<byte> realTimeInterpolations,
		        ref NativeArray<byte> moveImmediatelyMask,
		        float tickDelta,
		        ref NativeArray<byte> isMoving,
		        ref NativeArray<float> movementMultipliers,
		        ref StripedRingQueue<TickTransformProperties> transformProperties,
		        ref NativeArray<MoveRates> moveRates,
		        ref NativeArray<SetMoveRatesPayload> setMoveRatesPayloads,
		        ref NativeArray<SetMovementMultiplierPayload> setMovementMultiplierPayloads,
		        ref NativeArray<ClearTransformPropertiesQueuePayload> clearTransformPropertiesQueuePayloads)
		    {
		        MoveToTargetPayload jobPayload = jobPayloads[index];
		        if (jobPayload.executeMask == 0)
		            return;
		        jobPayloads[index] = new MoveToTargetPayload(0, default);

		        // We only need the delta once, then clear payload for this frame.
		        float remainingDelta = jobPayload.delta;

		        byte isOwner = useOwnerSettingsMask[index];
		        MovementSettings settings = isOwner != 0 ? ownerSettings[index] : spectatorSettings[index];
		        bool useLocalSpace = settings.UseLocalSpace;

		        while (remainingDelta > 0f)
		        {
		            int tpCount = transformProperties.GetCount(index);

		            // No data in queue.
		            if (tpCount == 0)
		                return;

		            byte realtimeInterpolation = realTimeInterpolations[index];
		            if (moveImmediatelyMask[index] != 0)
		            {
			            isMoving[index] = 1;
		            }
		            else
		            {
			            //Enough in buffer to move.
			            if (tpCount >= realtimeInterpolation)
			            {
				            isMoving[index] = 1;
			            }
			            else if (isMoving[index] == 0)
			            {
				            return;
			            }
			            /* If buffer is considerably under goal then halt
			             * movement. This will allow the buffer to grow. */
			            else if (tpCount - realtimeInterpolation < -4)
			            {
				            isMoving[index] = 0;
				            return;
			            }
		            }

		            TickTransformProperties ttp = transformProperties.Peek(index);
		            TransformPropertiesFlag smoothedProperties = settings.SmoothedProperties;

		            MoveRates moveRatesValue = moveRates[index];
		            float movementMultiplier = movementMultipliers[index];

		            moveRatesValue.Move(
		                graphicalTransform,
		                ttp.Properties,
		                smoothedProperties,
		                remainingDelta * movementMultiplier,
		                useWorldSpace: !useLocalSpace);
		            moveRates[index] = moveRatesValue;
		            
		            float tRemaining = moveRatesValue.TimeRemaining;

		            //if TimeRemaining is <= 0f then transform is at goal. Grab a new goal if possible.
		            if (tRemaining <= 0f)
		            {
			            //Dequeue current entry and if there's another call a move on it.
			            transformProperties.TryDequeue(index, out _);

			            //If there are entries left then setup for the next.
		                if (transformProperties.GetCount(index) > 0)
		                {
		                    setMoveRatesPayloads[index] = new SetMoveRatesPayload(1, ttp.Properties);
		                    SetMoveRatesJob.SetMoveRates(
		                        index,
		                        ref setMoveRatesPayloads,
		                        ref useOwnerSettingsMask,
		                        ref ownerSettings,
		                        ref spectatorSettings,
		                        ref transformProperties,
		                        tickDelta,
		                        ref moveRates,
		                        ref setMovementMultiplierPayloads);

		                    SetMovementMultiplierJob.SetMovementMultiplier(
		                        index,
		                        ref setMovementMultiplierPayloads,
		                        ref transformProperties,
		                        ref realTimeInterpolations,
		                        ref moveImmediatelyMask,
		                        ref movementMultipliers);

		                    // If there is leftover time, apply it to the next segment in this loop.
		                    if (tRemaining < 0f)
		                    {
		                        remainingDelta = Mathf.Abs(tRemaining);
		                        continue;
		                    }
		                }
		                //No remaining, set to snap.
		                else
		                {
		                    ClearTransformPropertiesQueueJob.ClearTransformPropertiesQueue(
		                        index,
		                        ref clearTransformPropertiesQueuePayloads,
		                        ref transformProperties,
		                        ref moveRates);
		                }
		            }

		            // Either we did not finish, or there is no leftover time to consume.
		            break;
		        }
		    }
		}

		[BurstCompile]
		private struct UpdateRealtimeInterpolationJob : IJobParallelFor
		{
			public NativeArray<UpdateRealtimeInterpolationPayload> jobPayloads;
			
			[ReadOnly] public NativeArray<byte> useOwnerSettingsMask;
			[ReadOnly] public NativeArray<MovementSettings> ownerSettings;
			[ReadOnly] public NativeArray<MovementSettings> spectatorSettings;
			
			[ReadOnly] public float tickDelta;
			[ReadOnly] public ushort tickRate;
			[ReadOnly] public uint localTick;
			[ReadOnly] public long rtt;
			[ReadOnly] public bool isServerOnlyStarted;
			
			public NativeArray<byte> realTimeInterpolations;

			public void Execute(int index)
			{
				UpdateRealtimeInterpolation(
					index,
					ref jobPayloads,
					ref useOwnerSettingsMask,
					ref ownerSettings,
					ref spectatorSettings,
					tickDelta,
					tickRate,
					localTick,
					rtt,
					isServerOnlyStarted,
					ref realTimeInterpolations);
			}

			/// <summary>
			/// Updates interpolation based on localClient latency when using adaptive interpolation, or uses set value when adaptive interpolation is off.
			/// </summary>
			public static void UpdateRealtimeInterpolation(
				int index,
				ref NativeArray<UpdateRealtimeInterpolationPayload> jobPayloads,
				ref NativeArray<byte> useOwnerSettingsMask,
				ref NativeArray<MovementSettings> ownerSettings,
				ref NativeArray<MovementSettings> spectatorSettings,
				float tickDelta,
				ushort tickRate,
				uint localTick,
				long rtt,
				bool isServerOnlyStarted,
				ref NativeArray<byte> realTimeInterpolations
				)
			{
				UpdateRealtimeInterpolationPayload jobPayload = jobPayloads[index];
				if (jobPayload.executeMask == 0)
					return;
				jobPayloads[index] = new UpdateRealtimeInterpolationPayload(0);
				
				byte isOwner = useOwnerSettingsMask[index];
				MovementSettings settings = isOwner != 0 ? ownerSettings[index] : spectatorSettings[index];
				
				/*  If not networked, server is started, or if not
				 * using adaptive interpolation then use
				 * flat interpolation.*/
				if (!GetUseAdaptiveInterpolation(settings, isServerOnlyStarted))
				{
					realTimeInterpolations[index] = settings.InterpolationValue;
					return;
				}

				/* If here then adaptive interpolation is being calculated. */
				
				//Calculate roughly what client state tick would be.
				//This should never be the case; this is a precautionary against underflow.
				if (localTick == TimeManager.UNSET_TICK)
					return;

				//Ensure at least 1 tick.
				uint rttTicks = TimeManager.TimeToTicks(rtt, tickDelta) + 1;

				uint clientStateTick = localTick - rttTicks;
				float interpolation = localTick - clientStateTick;

				//Minimum interpolation is that of adaptive interpolation level.
				interpolation += (byte)settings.AdaptiveInterpolationValue;

				//Ensure interpolation is not more than a second.
				if (interpolation > tickRate)
					interpolation = tickRate;
				else if (interpolation > byte.MaxValue)
					interpolation = byte.MaxValue;

				/* Only update realtime interpolation if it changed more than 1
				 * tick. This is to prevent excessive changing of interpolation value, which
				 * could result in noticeable speed ups/slow downs given movement multiplier
				 * may change when buffer is too full or short. */
				float realtimeInterpolation = realTimeInterpolations[index];
				if (realtimeInterpolation == 0 || math.abs(realtimeInterpolation - interpolation) > 1)
					realTimeInterpolations[index] = (byte)math.ceil(interpolation);
			}
			
			private static bool GetUseAdaptiveInterpolation(in MovementSettings settings, in bool isServerOnlyStarted)
			{
				if (settings.AdaptiveInterpolationValue == AdaptiveInterpolationType.Off || isServerOnlyStarted)
					return false;

				return true;
			}
		}

		[BurstCompile]
		private struct DiscardExcessiveTransformPropertiesQueueJob : IJobParallelFor
		{
			public NativeArray<DiscardExcessiveTransformPropertiesQueuePayload> jobPayloads;
			
			[ReadOnly] public NativeArray<byte> realTimeInterpolations;
			[ReadOnly] public int requiredQueuedOverInterpolation;
			
			public StripedRingQueue<TickTransformProperties> transformProperties;
			[WriteOnly] public NativeArray<SetMoveRatesPayload> setMoveRatesPayloads;
			
			public void Execute(int index)
			{
				DiscardExcessiveTransformPropertiesQueue(
					index,
					ref jobPayloads,
					ref realTimeInterpolations,
					requiredQueuedOverInterpolation,
					ref transformProperties,
					ref setMoveRatesPayloads);
			}

			/// <summary>
			/// Discards datas over interpolation limit from movement queue.
			/// </summary>
			public static void DiscardExcessiveTransformPropertiesQueue(
				int index,
				ref NativeArray<DiscardExcessiveTransformPropertiesQueuePayload> jobPayloads,
				ref NativeArray<byte> realTimeInterpolations,
				int requiredQueuedOverInterpolation,
				ref StripedRingQueue<TickTransformProperties> transformProperties,
				ref NativeArray<SetMoveRatesPayload> setMoveRatesPayloads)
			{
				setMoveRatesPayloads[index] = new SetMoveRatesPayload(0, default);
				
				DiscardExcessiveTransformPropertiesQueuePayload jobPayload = jobPayloads[index];
				if (jobPayload.executeMask == 0)
					return;
				jobPayloads[index] = new DiscardExcessiveTransformPropertiesQueuePayload(0);
				
				int propertiesCount = transformProperties.GetCount(index);
				int realtimeInterpolationValue = realTimeInterpolations[index];
				int dequeueCount = propertiesCount - (realtimeInterpolationValue + requiredQueuedOverInterpolation);

				// If there are entries to dequeue.
				if (dequeueCount > 0)
				{
					TickTransformProperties ttp;
					transformProperties.DequeueUpTo(index, dequeueCount, out ttp);

					var nextValues = ttp.Properties;
					setMoveRatesPayloads[index] = new SetMoveRatesPayload(1, nextValues);
				}
			}
		}
		
		[BurstCompile]
		private struct SetMoveRatesJob : IJobParallelFor
		{
			public NativeArray<SetMoveRatesPayload> jobPayloads;
			
			[ReadOnly] public NativeArray<byte> useOwnerSettingsMask;
			[ReadOnly] public NativeArray<MovementSettings> ownerSettings;
			[ReadOnly] public NativeArray<MovementSettings> spectatorSettings;
			
			public StripedRingQueue<TickTransformProperties> transformProperties;
			[ReadOnly] public float tickDelta;
			
			[WriteOnly] public NativeArray<MoveRates> moveRates;
			[WriteOnly] public NativeArray<SetMovementMultiplierPayload> setMovementMultiplierPayloads;
			
			public void Execute(int index)
			{
				SetMoveRates(
					index,
					ref jobPayloads,
					ref useOwnerSettingsMask,
					ref ownerSettings,
					ref spectatorSettings,
					ref transformProperties,
					tickDelta,
					ref moveRates,
					ref setMovementMultiplierPayloads);
			}

			/// <summary>
			/// Sets new rates based on next entries in transformProperties queue, against a supplied TransformProperties.
			/// </summary>
			public static void SetMoveRates(
				int index,
				ref NativeArray<SetMoveRatesPayload> jobPayloads,
				ref NativeArray<byte> useOwnerSettingsMask,
				ref NativeArray<MovementSettings> ownerSettings,
				ref NativeArray<MovementSettings> spectatorSettings,
				ref StripedRingQueue<TickTransformProperties> transformProperties,
				float tickDelta,
				ref NativeArray<MoveRates> moveRates,
				ref NativeArray<SetMovementMultiplierPayload> setMovementMultiplierPayloads)
			{
				moveRates[index] = new(MoveRates.UNSET_VALUE);
				setMovementMultiplierPayloads[index] = new SetMovementMultiplierPayload(0);
				
				SetMoveRatesPayload jobPayload = jobPayloads[index];
				if (jobPayload.executeMask == 0 || transformProperties.GetCount(index) == 0)
					return;
				jobPayloads[index] = new SetMoveRatesPayload(0, default);

				TransformProperties prevValues = jobPayload.prevValues;
				TransformProperties nextValues = transformProperties.Peek(index).Properties;
				float duration = tickDelta;

				byte isOwner = useOwnerSettingsMask[index];
				MovementSettings settings = isOwner != 0 ? ownerSettings[index] : spectatorSettings[index];
				float teleportThreshold = settings.EnableTeleport
					? settings.TeleportThreshold * settings.TeleportThreshold
					: MoveRates.UNSET_VALUE;
				
				MoveRates moveRatesValue = MoveRates.GetMoveRates(prevValues, nextValues, duration, teleportThreshold);
				moveRatesValue.TimeRemaining = duration;
				moveRates[index] = moveRatesValue;
				setMovementMultiplierPayloads[index] = new SetMovementMultiplierPayload(1);
			}
		}
		
		[BurstCompile]
		private struct SetMovementMultiplierJob : IJobParallelFor
		{
			public NativeArray<SetMovementMultiplierPayload> jobPayloads;
			
			public StripedRingQueue<TickTransformProperties> transformProperties;
			[ReadOnly] public NativeArray<byte> realTimeInterpolations;
			[ReadOnly] public NativeArray<byte> moveImmediatelyMask;
			
			public NativeArray<float> movementMultipliers;
			
			public void Execute(int index)
			{
				SetMovementMultiplier(
					index,
					ref jobPayloads,
					ref transformProperties,
					ref realTimeInterpolations,
					ref moveImmediatelyMask,
					ref movementMultipliers);
			}

			public static void SetMovementMultiplier(
				int index,
				ref NativeArray<SetMovementMultiplierPayload> jobPayloads,
				ref StripedRingQueue<TickTransformProperties> transformProperties,
				ref NativeArray<byte> realTimeInterpolations,
				ref NativeArray<byte> moveImmediatelyMask,
				ref NativeArray<float> movementMultipliers)
			{
				SetMovementMultiplierPayload jobPayload = jobPayloads[index];
				if (jobPayload.executeMask == 0)
					return;
				jobPayloads[index] = new SetMovementMultiplierPayload(0);
				
				byte moveImmediately = moveImmediatelyMask[index];
				byte realTimeInterpolation = realTimeInterpolations[index];
				float movementMultiplier = movementMultipliers[index];
				int propertiesCount = transformProperties.GetCount(index);
				if (moveImmediately != 0)
				{
					float percent = math.unlerp(0, realTimeInterpolation, propertiesCount);
					movementMultiplier = percent;

					movementMultiplier = math.clamp(movementMultiplier, 0.5f, 1.05f);
				}
				// For the time being, not moving immediately uses these multiplier calculations.
				else
				{
					/* If there's more in queue than interpolation then begin to move faster based on overage.
					 * Move 5% faster for every overage. */
					int overInterpolation = propertiesCount - realTimeInterpolation;
					// If needs to be adjusted.
					if (overInterpolation != 0)
					{
						movementMultiplier += 0.015f * overInterpolation;
					}
					// If does not need to be adjusted.
					else
					{
						// If interpolation is 1 then slow down just barely to accomodate for frame delta variance.
						if (realTimeInterpolation == 1)
							movementMultiplier = 1f;
					}

					movementMultiplier = math.clamp(movementMultiplier, 0.95f, 1.05f);
				}

				movementMultipliers[index] = movementMultiplier;
			}
		}
		
		[BurstCompile]
		private struct AddTransformPropertiesJob : IJobParallelForTransform
		{
			public NativeArray<AddTransformPropertiesPayload> jobPayloads;
			
			[ReadOnly] public NativeArray<byte> useOwnerSettingsMask;
			[ReadOnly] public NativeArray<MovementSettings> ownerSettings;
			[ReadOnly] public NativeArray<MovementSettings> spectatorSettings;
			public StripedRingQueue<TickTransformProperties> transformProperties;
			[WriteOnly] public NativeArray<SetMoveRatesPayload> setMoveRatesPayloads;
			
			public void Execute(int index, TransformAccess graphicalTransform)
			{
				AddTransformProperties(
					index, 
					graphicalTransform,
					ref jobPayloads, 
					ref useOwnerSettingsMask,
					ref ownerSettings,
					ref spectatorSettings,
					ref transformProperties, 
					ref setMoveRatesPayloads);
			}

			/// <summary>
			/// Adds a new transform properties and sets move rates if needed.
			/// </summary>
			public static void AddTransformProperties(
				int index,
				TransformAccess graphicalTransform,
				ref NativeArray<AddTransformPropertiesPayload> jobPayloads,
				ref NativeArray<byte> useOwnerSettingsMask,
				ref NativeArray<MovementSettings> ownerSettings,
				ref NativeArray<MovementSettings> spectatorSettings,
				ref StripedRingQueue<TickTransformProperties> transformProperties,
				ref NativeArray<SetMoveRatesPayload> setMoveRatesPayloads)
			{
				setMoveRatesPayloads[index] = new SetMoveRatesPayload(0, default);
				
				AddTransformPropertiesPayload jobPayload = jobPayloads[index];
				if (jobPayload.executeMask == 0)
					return;
				jobPayloads[index] = new AddTransformPropertiesPayload(0, default);
				
				transformProperties.Enqueue(index, jobPayload.tickTransformProperties);

				//If first entry then set move rates.
				if (transformProperties.GetCount(index) == 1)
				{
					byte isOwner = useOwnerSettingsMask[index];
					MovementSettings settings = isOwner != 0 ? ownerSettings[index] : spectatorSettings[index];
					
					bool useLocalSpace = settings.UseLocalSpace;
					TransformProperties gfxProperties =  GetTransformProperties(graphicalTransform, useLocalSpace);
					setMoveRatesPayloads[index] = new SetMoveRatesPayload(1, gfxProperties);
				}
			}
		}
		
		[BurstCompile]
		private struct ClearTransformPropertiesQueueJob : IJobParallelFor
		{
			public NativeArray<ClearTransformPropertiesQueuePayload> jobPayloads;
			public StripedRingQueue<TickTransformProperties> transformProperties;
			[WriteOnly] public NativeArray<MoveRates> moveRates;
			
			public void Execute(int index)
			{
				ClearTransformPropertiesQueue(
					index,
					ref jobPayloads,
					ref transformProperties, 
					ref moveRates);
			}

			/// <summary>
			/// Clears the pending movement queue.
			/// </summary>
			public static void ClearTransformPropertiesQueue(
				int index,
				ref NativeArray<ClearTransformPropertiesQueuePayload> jobPayloads,
				ref StripedRingQueue<TickTransformProperties> transformProperties,
				ref NativeArray<MoveRates> moveRates)
			{
				ClearTransformPropertiesQueuePayload jobPayload = jobPayloads[index];
				if (jobPayload.executeMask == 0)
					return;
				jobPayloads[index] = new ClearTransformPropertiesQueuePayload(0);
				
				transformProperties.Clear(index);
				//Also unset move rates since there is no more queue.
				moveRates[index] = new MoveRates(MoveRates.UNSET_VALUE);
			}
		}
		
		[BurstCompile]
		private struct ModifyTransformPropertiesJob : IJobParallelForTransform
		{
			public NativeArray<ModifyTransformPropertiesPayload> jobPayloads;
			[ReadOnly] public NativeArray<byte> detachOnStartMask;
			[ReadOnly] public NativeArray<byte> useOwnerSettingsMask;
			[ReadOnly] public NativeArray<MovementSettings> ownerSettings;
			[ReadOnly] public NativeArray<MovementSettings> spectatorSettings;
			[ReadOnly] public NativeArray<TransformProperties> targetSnapshot;
			
			public StripedRingQueue<TickTransformProperties> transformProperties;
			
			public void Execute(int index, TransformAccess trackerTransform)
			{
				ModifyTransformProperties(
					index,
					trackerTransform,
					ref jobPayloads,
					ref detachOnStartMask,
					ref useOwnerSettingsMask,
					ref ownerSettings,
					ref spectatorSettings,
					ref targetSnapshot,
					ref transformProperties);
			}

			/// <summary>
			/// Modifies a transform property for a tick. This does not error check for empty collections.
			/// firstTick - First tick in the queue. If 0 this will be looked up.
			/// </summary>
			public static void ModifyTransformProperties(
				int index,
				TransformAccess trackerTransform,
				ref NativeArray<ModifyTransformPropertiesPayload> jobPayloads,
				ref NativeArray<byte> detachOnStartMask,
				ref NativeArray<byte> useOwnerSettingsMask,
				ref NativeArray<MovementSettings> ownerSettings,
				ref NativeArray<MovementSettings> spectatorSettings,
				ref NativeArray<TransformProperties> targetSnapshot,
				ref StripedRingQueue<TickTransformProperties> transformProperties)
			{
				ModifyTransformPropertiesPayload jobPayload = jobPayloads[index];
				if (jobPayload.executeMask == 0)
					return;
				jobPayloads[index] = new ModifyTransformPropertiesPayload(0, default, default);
				
				int queueCount = transformProperties.GetCount(index);
                uint tick = jobPayload.clientTick;
                /*Ticks will always be added incremental by 1 so it's safe to jump ahead the difference
                 * of tick and firstTick. */
                int tickIndex = (int)(tick - jobPayload.firstTick);
                //Replace with new data.
                if (tickIndex < queueCount)
                {
	                TickTransformProperties tickTransformProperties = transformProperties[index, tickIndex];
                    if (tick != tickTransformProperties.Tick)
                    {
                        //Should not be possible.
                    }
                    else
                    {
	                    bool isDetach = detachOnStartMask[index] != 0;
	                    byte isOwner = useOwnerSettingsMask[index];
	                    MovementSettings settings = isOwner != 0 ? ownerSettings[index] : spectatorSettings[index];
	                    
	                    bool useLocalSpace = settings.UseLocalSpace;
	                    TransformProperties newProperties = GetTrackerTransformProperties(trackerTransform, isDetach, useLocalSpace);
	                    if (useLocalSpace) newProperties += targetSnapshot[index];
                        /* Adjust transformProperties to ease into any corrections.
                         * The corrected value is used the more the index is to the end
                         * of the queue. */
                        /* We want to be fully eased in by the last entry of the queue. */

                        int lastPossibleIndex = queueCount - 1;
                        int adjustedQueueCount = lastPossibleIndex - 1;
                        if (adjustedQueueCount < 1)
                            adjustedQueueCount = 1;
                        float easePercent = (float)tickIndex / adjustedQueueCount;

                        //If easing.
                        if (easePercent < 1f)
                        {
                            if (easePercent < 1f)
                                easePercent = (float)Math.Pow(easePercent, adjustedQueueCount - tickIndex);

                            TransformProperties oldProperties = tickTransformProperties.Properties;
                            newProperties.Position = Vector3.Lerp(oldProperties.Position, newProperties.Position, easePercent);
                            newProperties.Rotation = Quaternion.Lerp(oldProperties.Rotation, newProperties.Rotation, easePercent);
                            newProperties.Scale = Vector3.Lerp(oldProperties.Scale, newProperties.Scale, easePercent);
                        }

                        transformProperties[index, tickIndex] = new TickTransformProperties(tick, newProperties);
                    }
                }
                else
                {
                    //This should never happen.
                }
			}
		}
		
		[BurstCompile]
		private struct SnapNonSmoothedPropertiesJob : IJobParallelForTransform
		{
			public NativeArray<SnapNonSmoothedPropertiesPayload> jobPayloads;
			
			[ReadOnly] public NativeArray<byte> useOwnerSettingsMask;
			[ReadOnly] public NativeArray<MovementSettings> ownerSettings;
			[ReadOnly] public NativeArray<MovementSettings> spectatorSettings;
			
			public void Execute(int index, TransformAccess graphicalTransform)
			{
				SnapNonSmoothedProperties(
					index,
					graphicalTransform,
					ref jobPayloads,
					ref useOwnerSettingsMask,
					ref ownerSettings,
					ref spectatorSettings);
			}

			/// <summary>
			/// Snaps non-smoothed properties to original positoin if setting is enabled.
			/// </summary>
			public static void SnapNonSmoothedProperties(
				int index,
				TransformAccess graphicalTransform,
				ref NativeArray<SnapNonSmoothedPropertiesPayload> jobPayloads,
				ref NativeArray<byte> useOwnerSettingsMask,
				ref NativeArray<MovementSettings> ownerSettings,
				ref NativeArray<MovementSettings> spectatorSettings)
			{
				SnapNonSmoothedPropertiesPayload jobPayload = jobPayloads[index];
				if (jobPayload.executeMask == 0)
					return;
				jobPayloads[index] = new SnapNonSmoothedPropertiesPayload(0, default);
				
				byte isOwner = useOwnerSettingsMask[index];
				MovementSettings settings = isOwner != 0 ? ownerSettings[index] : spectatorSettings[index];
				bool useLocalSpace = settings.UseLocalSpace;

				bool snapNonSmoothedProperties = settings.SnapNonSmoothedProperties;
				//Feature is not enabled.
				if (!snapNonSmoothedProperties)
					return;

				TransformPropertiesFlag smoothedProperties = settings.SmoothedProperties;

				//Everything is smoothed.
				if (smoothedProperties == TransformPropertiesFlag.Everything)
					return;

				TransformProperties goalValues = jobPayload.goalValues;

				if (!smoothedProperties.FastContains(TransformPropertiesFlag.Position))
				{
					if (useLocalSpace)
						graphicalTransform.localPosition = goalValues.Position;
					else
						graphicalTransform.position = goalValues.Position;
				}

				if (!smoothedProperties.FastContains(TransformPropertiesFlag.Rotation))
				{
					if (useLocalSpace)
						graphicalTransform.localRotation = goalValues.Rotation;
					else
						graphicalTransform.rotation = goalValues.Rotation;
				}

				if (!smoothedProperties.FastContains(TransformPropertiesFlag.Scale))
					graphicalTransform.localScale = goalValues.Scale;
			}
		}
		
		[BurstCompile]
		private struct TeleportJob : IJobParallelForTransform
		{ 
			public NativeArray<TeleportPayload> jobPayloads;
			
			[ReadOnly] public NativeArray<byte> useOwnerSettingsMask;
			[ReadOnly] public NativeArray<MovementSettings> ownerSettings;
			[ReadOnly] public NativeArray<MovementSettings> spectatorSettings;
			[ReadOnly] public NativeArray<TransformProperties> preTickTrackerSnapshot;
			[ReadOnly] public uint localTick;

			public StripedRingQueue<TickTransformProperties> transformProperties;
			public NativeArray<ClearTransformPropertiesQueuePayload> clearTransformPropertiesQueuePayloads;
			[WriteOnly] public NativeArray<MoveRates> moveRates;
			[WriteOnly] public NativeArray<uint> teleportedTick;
			
			public void Execute(int index, TransformAccess graphicalTransform)
			{
				Teleport(
					index,
					graphicalTransform,
					ref jobPayloads,
					ref useOwnerSettingsMask,
					ref ownerSettings,
					ref spectatorSettings,
					ref preTickTrackerSnapshot,
					localTick,
					ref transformProperties,
					ref clearTransformPropertiesQueuePayloads,
					ref moveRates,
					ref teleportedTick);
			}

			/// <summary>
			/// Snaps non-smoothed properties to original positoin if setting is enabled.
			/// </summary>
			public static void Teleport(
				int index,
				TransformAccess graphicalTransform,
				ref NativeArray<TeleportPayload> jobPayloads,
				ref NativeArray<byte> useOwnerSettingsMask,
				ref NativeArray<MovementSettings> ownerSettings,
				ref NativeArray<MovementSettings> spectatorSettings,
				ref NativeArray<TransformProperties> preTickTrackerSnapshot,
				uint localTick,
				ref StripedRingQueue<TickTransformProperties> transformProperties,
				ref NativeArray<ClearTransformPropertiesQueuePayload> clearTransformPropertiesQueuePayloads,
				ref NativeArray<MoveRates> moveRates,
				ref NativeArray<uint> teleportedTick)
			{
				TeleportPayload jobPayload = jobPayloads[index];
				if (jobPayload.executeMask == 0)
					return;
				jobPayloads[index] = new TeleportPayload(0);
				
				byte isOwner = useOwnerSettingsMask[index];
				MovementSettings settings = isOwner != 0 ? ownerSettings[index] : spectatorSettings[index];
				bool useLocalSpace = settings.UseLocalSpace;

				AdaptiveInterpolationType adaptiveInterpolationValue = settings.AdaptiveInterpolationValue;

				//If using adaptive interpolation then set the tick which was teleported.
				if (adaptiveInterpolationValue != AdaptiveInterpolationType.Off)
					teleportedTick[index] = localTick;

				clearTransformPropertiesQueuePayloads[index] = new ClearTransformPropertiesQueuePayload(1);
				ClearTransformPropertiesQueueJob.ClearTransformPropertiesQueue(
					index,
					ref clearTransformPropertiesQueuePayloads,
					ref transformProperties,
					ref moveRates);
				
				TransformProperties trackerProps = preTickTrackerSnapshot[index];
				SetTransformProperties(graphicalTransform, trackerProps, useLocalSpace);
			}
		}
		
		/// <summary>
		/// Gets properties for the graphical transform in the desired space.
		/// </summary>
		[MethodImpl(MethodImplOptions.AggressiveInlining)]
		private static TransformProperties GetTransformProperties(TransformAccess transform, bool useLocalSpace)
		{
			if (useLocalSpace)
				return transform.GetLocalProperties();
			else
				return transform.GetWorldProperties();
		}
		
		/// <summary>
		/// Gets properties for the tracker transform in the desired space, accounting for detach.
		/// </summary>
		[MethodImpl(MethodImplOptions.AggressiveInlining)]
		private static TransformProperties GetTrackerTransformProperties(TransformAccess trackerTransform, bool isDetach, bool useLocalSpace)
		{
			/* Return lossyScale if graphical is not attached. Otherwise,
			 * graphical should retain the tracker localScale so it changes
			 * with root. */
			
			Vector3 scale = isDetach ? ExtractLossyScale(trackerTransform) : trackerTransform.localScale;
			Vector3 pos;
			Quaternion rot;
			
			if (useLocalSpace)
				trackerTransform.GetCorrectLocalPositionAndRotation(out pos, out rot);
			else
				trackerTransform.GetPositionAndRotation(out pos, out rot);
			
			return new TransformProperties(pos, rot, scale);
		}
		
		/// <summary>
		/// Applies properties to a transform using the desired space for position/rotation.
		/// </summary>
		[MethodImpl(MethodImplOptions.AggressiveInlining)]
		private static void SetTransformProperties(TransformAccess transform, in TransformProperties properties, bool useLocalSpace)
		{
			if (useLocalSpace)
				transform.SetLocalProperties(properties);
			else
				transform.SetWorldProperties(properties);
		}
		
		[MethodImpl(MethodImplOptions.AggressiveInlining)]
		private static Vector3 ExtractLossyScale(TransformAccess transform)
		{
			var m = transform.localToWorldMatrix;
			var c0 = new float3(m.m00, m.m10, m.m20);
			var c1 = new float3(m.m01, m.m11, m.m21);
			var c2 = new float3(m.m02, m.m12, m.m22);
			return new Vector3(math.length(c0), math.length(c1), math.length(c2));
		}
		
		#endregion
		
		
		public static TransformProperties GetGraphicalWorldProperties(TransformAccess graphicalTransform)
		{
			return graphicalTransform.GetWorldProperties();
		}
		
		public static TransformProperties GetTrackerWorldProperties(TransformAccess trackerTransform, bool isDetach)
		{
			/* Return lossyScale if graphical is not attached. Otherwise,
			 * graphical should retain the tracker localScale so it changes
			 * with root. */
			
			trackerTransform.GetPositionAndRotation(out var pos, out var rot);
			Vector3 scl;
			if (isDetach)
			{
				var m = trackerTransform.localToWorldMatrix;
				var c0 = new float3(m.m00, m.m10, m.m20);
				var c1 = new float3(m.m01, m.m11, m.m21);
				var c2 = new float3(m.m02, m.m12, m.m22);
				scl = new Vector3(math.length(c0), math.length(c1), math.length(c2));
			}
			else scl = trackerTransform.localScale;
			return new TransformProperties(pos, rot, scl);
		}
	}
}
#endif