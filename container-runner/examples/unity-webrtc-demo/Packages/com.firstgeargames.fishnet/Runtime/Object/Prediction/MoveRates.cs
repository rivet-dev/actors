using GameKit.Dependencies.Utilities;
using Unity.Profiling;
using UnityEngine;
using UnityEngine.Scripting;

namespace FishNet.Object.Prediction
{
    /// <summary>
    /// Used to make calculations and perform actions in moving transforms over time.
    /// </summary>
    [Preserve]
    public struct MoveRates
    {
        /// <summary>
        /// Rate at which to move Position.
        /// </summary>
        public float Position;
        /// <summary>
        /// Rate at which to move Rotation.
        /// </summary>
        public float Rotation;
        /// <summary>
        /// Rate at which to move Scale.
        /// </summary>
        public float Scale;
        /// <summary>
        /// Time remaining until the move is complete.
        /// </summary>
        public float TimeRemaining;
        /// <summary>
        /// Value used when data is not set.
        /// </summary>
        public const float UNSET_VALUE = float.NegativeInfinity;
        /// <summary>
        /// Value used when move rate should be instant.
        /// </summary>
        public const float INSTANT_VALUE = float.PositiveInfinity;
        /// <summary>
        /// True if any data is set. Once set, this will remain true until ResetState is called.
        /// </summary>
        public bool IsValid { get; private set; }

        public MoveRates(float value) : this()
        {
            Position = value;
            Rotation = value;
            Scale = value;

            IsValid = true;
        }

        public MoveRates(float position, float rotation) : this()
        {
            Position = position;
            Rotation = rotation;
            Scale = INSTANT_VALUE;

            IsValid = true;
        }

        public MoveRates(float position, float rotation, float scale) : this()
        {
            Position = position;
            Rotation = rotation;
            Scale = scale;

            IsValid = true;
        }

        public MoveRates(float position, float rotation, float scale, float timeRemaining)
        {
            Position = position;
            Rotation = rotation;
            Scale = scale;
            TimeRemaining = timeRemaining;

            IsValid = true;
        }

        /// <summary>
        /// True if a positional move rate is set.
        /// </summary>
        public bool IsPositionSet => Position != UNSET_VALUE;
        /// <summary>
        /// True if rotation move rate is set.
        /// </summary>
        public bool IsRotationSet => Rotation != UNSET_VALUE;
        /// <summary>
        /// True if a scale move rate is set.
        /// </summary>
        public bool IsScaleSet => Scale != UNSET_VALUE;
        /// <summary>
        /// True if position move rate should be instant.
        /// </summary>
        public bool IsPositionInstantValue => Position == INSTANT_VALUE;
        /// <summary>
        /// True if rotation move rate should be instant.
        /// </summary>
        public bool IsRotationInstantValue => Rotation == INSTANT_VALUE;
        /// <summary>
        /// True if scale move rate should be instant.
        /// </summary>
        public bool IsScaleInstantValue => Scale == INSTANT_VALUE;

        #region Private Profiler Markers
        private static readonly ProfilerMarker _pm_GetMoveRatesFull = new("MoveRates.GetMoveRates(Vector3, Vector3, Quaternion, Quaternion, Vector3, Vector3, float, float)");
        private static readonly ProfilerMarker _pm_GetMoveRatesVec = new("MoveRates.GetMoveRates(Vector3, Vector3, float, float)");
        private static readonly ProfilerMarker _pm_Move = new("MoveRates.Move(Transform, TransformPropertiesFlag, Vector3, float, Quaternion, float, Vector3, float, float, bool)");
        #endregion

        /// <summary>
        /// Sets all rates to instant.
        /// </summary>
        public void SetInstantRates() => Update(INSTANT_VALUE);

        /// <summary>
        /// Sets all rates to the same value.
        /// </summary>
        public void Update(float value) => Update(value, value, value);

        /// <summary>
        /// Sets rates for each property.
        /// </summary>
        public void Update(float position, float rotation, float scale)
        {
            Position = position;
            Rotation = rotation;
            Scale = scale;

            IsValid = true;
        }

        /// <summary>
        /// Sets rates for each property.
        /// </summary>
        public void Update(float position, float rotation, float scale, float timeRemaining)
        {
            Position = position;
            Rotation = rotation;
            Scale = scale;
            TimeRemaining = timeRemaining;

            IsValid = true;
        }

        /// <summary>
        /// Updates to new values.
        /// </summary>
        public void Update(MoveRates moveRates) => Update(moveRates.Position, moveRates.Rotation, moveRates.Scale, moveRates.TimeRemaining);

        /// <summary>
        /// Updates to new values.
        /// </summary>
        public void Update(MoveRatesCls moveRates) => Update(moveRates.Position, moveRates.Rotation, moveRates.Scale, moveRates.TimeRemaining);

        /// <summary>
        /// Resets to unset values.
        /// </summary>
        public void ResetState()
        {
            Update(UNSET_VALUE, UNSET_VALUE, UNSET_VALUE, timeRemaining: 0f);

            IsValid = false;
        }

        /// <summary>
        /// Returns a new MoveRates based on previous values, and a transforms current position.
        /// </summary>
        public static MoveRates GetWorldMoveRates(Transform from, Transform to, float duration, float teleportThreshold)
        {
            return GetMoveRates(from.position, to.position, from.rotation, to.rotation, from.localScale, to.localScale, duration, teleportThreshold);
        }

        /// <summary>
        /// Returns a new MoveRates based on previous values, and a transforms current position.
        /// </summary>
        public static MoveRates GetLocalMoveRates(Transform from, Transform to, float duration, float teleportThreshold)
        {
            return GetMoveRates(from.localPosition, to.localPosition, from.localRotation, to.localRotation, from.localScale, to.localScale, duration, teleportThreshold);
        }

        /// <summary>
        /// Returns a new MoveRates based on previous values, and a transforms current position.
        /// </summary>
        public static MoveRates GetWorldMoveRates(TransformProperties prevValues, Transform t, float duration, float teleportThreshold)
        {
            return GetMoveRates(prevValues.Position, t.position, prevValues.Rotation, t.rotation, prevValues.Scale, t.localScale, duration, teleportThreshold);
        }

        /// <summary>
        /// Returns a new MoveRates based on previous values, and a transforms current position.
        /// </summary>
        public static MoveRates GetLocalMoveRates(TransformProperties prevValues, Transform t, float duration, float teleportThreshold)
        {
            return GetMoveRates(prevValues.Position, t.localPosition, prevValues.Rotation, t.localRotation, prevValues.Scale, t.localScale, duration, teleportThreshold);
        }

        /// <summary>
        /// Returns a new MoveRates based on previous values, and a transforms current position.
        /// </summary>
        public static MoveRates GetMoveRates(TransformProperties prevValues, TransformProperties nextValues, float duration, float teleportThreshold)
        {
            return GetMoveRates(prevValues.Position, nextValues.Position, prevValues.Rotation, nextValues.Rotation, prevValues.Scale, nextValues.Scale, duration, teleportThreshold);
        }

        /// <summary>
        /// Returns a new MoveRates based on previous values, and a transforms current position.
        /// </summary>
        public static MoveRates GetMoveRates(Vector3 fromPosition, Vector3 toPosition, Quaternion fromRotation, Quaternion toRotation, Vector3 fromScale, Vector3 toScale, float duration, float teleportThreshold)
        {
            using (_pm_GetMoveRatesFull.Auto())
            {
                float rate;

                /* Position. */
                rate = toPosition.GetRate(fromPosition, duration, out float distance);
                // Basic teleport check.
                if (teleportThreshold != UNSET_VALUE && distance > teleportThreshold)
                    return new(INSTANT_VALUE, INSTANT_VALUE, INSTANT_VALUE, duration);

                //Smoothing.
                float positionRate = rate.SetIfUnderTolerance(0.0001f, INSTANT_VALUE);
                rate = toRotation.GetRate(fromRotation, duration, out _);
                float rotationRate = rate.SetIfUnderTolerance(0.2f, INSTANT_VALUE);
                rate = toScale.GetRate(fromScale, duration, out _);
                float scaleRate = rate.SetIfUnderTolerance(0.0001f, INSTANT_VALUE);

                return new(positionRate, rotationRate, scaleRate, duration);
            }
        }

        /// <summary>
        /// Gets a move rate for two Vector3s.
        /// </summary>
        public static float GetMoveRate(Vector3 fromPosition, Vector3 toPosition, float duration, float teleportThreshold)
        {
            using (_pm_GetMoveRatesVec.Auto())
            {
                float rate;
                float distance;

                /* Position. */
                rate = toPosition.GetRate(fromPosition, duration, out distance);
                //Basic teleport check.
                if (teleportThreshold != UNSET_VALUE && distance > teleportThreshold)
                {
                    return INSTANT_VALUE;
                }
                //Smoothing.
                else
                {
                    float positionRate = rate.SetIfUnderTolerance(0.0001f, INSTANT_VALUE);
                    return positionRate;
                }
            }
        }

        /// <summary>
        /// Gets a move rate for two Quaternions.
        /// </summary>
        public static float GetMoveRate(Quaternion fromRotation, Quaternion toRotation, float duration)
        {
            float rate = toRotation.GetRate(fromRotation, duration, out _);
            float rotationRate = rate.SetIfUnderTolerance(0.2f, INSTANT_VALUE);
            return rotationRate;
        }

        /// <summary>
        /// Moves transform to target values.
        /// </summary>
        public void Move(Transform movingTransform, TransformProperties goalProperties, float delta, bool useWorldSpace)
        {
            if (!IsValid)
                return;

            Move(movingTransform, TransformPropertiesFlag.Everything, goalProperties.Position, Position, goalProperties.Rotation, Rotation, goalProperties.Scale, Scale, delta, useWorldSpace);
            TimeRemaining -= delta;
        }

        /// <summary>
        /// Moves transform to target values.
        /// </summary>
        public void Move(Transform movingTransform, TransformProperties goalProperties, TransformPropertiesFlag movedProperties, float delta, bool useWorldSpace)
        {
            if (!IsValid)
                return;

            Move(movingTransform, movedProperties, goalProperties.Position, Position, goalProperties.Rotation, Rotation, goalProperties.Scale, Scale, delta, useWorldSpace);
            TimeRemaining -= delta;
        }

        /// <summary>
        /// Moves transform to target values.
        /// </summary>
        public static void Move(Transform movingTransform, TransformPropertiesFlag movedProperties, Vector3 posGoal, float posRate, Quaternion rotGoal, float rotRate, Vector3 scaleGoal, float scaleRate, float delta, bool useWorldSpace)
        {
            using (_pm_Move.Auto())
            {
                Transform t = movingTransform;

                bool containsPosition = movedProperties.FastContains(TransformPropertiesFlag.Position);
                bool containsRotation = movedProperties.FastContains(TransformPropertiesFlag.Rotation);
                bool containsScale = movedProperties.FastContains(TransformPropertiesFlag.Scale);

                //World space.
                if (useWorldSpace)
                {
                    if (containsPosition)
                    {
                        if (posRate == INSTANT_VALUE)
                        {
                            t.position = posGoal;
                        }
                        else if (posRate == UNSET_VALUE) { }
                        else
                        {
                            t.position = Vector3.MoveTowards(t.position, posGoal, posRate * delta);
                        }
                    }

                    if (containsRotation)
                    {
                        if (rotRate == INSTANT_VALUE)
                        {
                            t.rotation = rotGoal;
                        }
                        else if (rotRate == UNSET_VALUE) { }
                        else
                        {
                            t.rotation = Quaternion.RotateTowards(t.rotation, rotGoal, rotRate * delta);
                        }
                    }
                }
                //Local space.
                else
                {
                    if (containsPosition)
                    {
                        if (posRate == INSTANT_VALUE)
                        {
                            t.localPosition = posGoal;
                        }
                        else if (posRate == UNSET_VALUE) { }
                        else
                        {
                            t.localPosition = Vector3.MoveTowards(t.localPosition, posGoal, posRate * delta);
                        }
                    }

                    if (containsRotation)
                    {
                        if (rotRate == INSTANT_VALUE)
                        {
                            t.localRotation = rotGoal;
                        }
                        else if (rotRate == UNSET_VALUE) { }
                        else
                        {
                            t.localRotation = Quaternion.RotateTowards(t.localRotation, rotGoal, rotRate * delta);
                        }
                    }
                }

                //Scale always uses local.
                if (containsScale)
                {
                    if (scaleRate == INSTANT_VALUE)
                    {
                        t.localScale = scaleGoal;
                    }
                    else if (scaleRate == UNSET_VALUE) { }
                    else
                    {
                        t.localScale = Vector3.MoveTowards(t.localScale, scaleGoal, scaleRate * delta);
                    }
                }
            }
        }
    }

    /// <summary>
    /// Used to make calculations and perform actions in moving transforms over time.
    /// </summary>
    /// <remarks>This acts as a wrapper for MoveRates struct.</remarks>
    public class MoveRatesCls : IResettable
    {
        /// <summary>
        /// Container of all move rate information.
        /// </summary>
        private MoveRates _moveRates = new();
        /// <summary>
        /// Rate at which to move Position.
        /// </summary>
        public float Position => _moveRates.Position;
        /// <summary>
        /// Rate at which to move Rotation.
        /// </summary>
        public float Rotation => _moveRates.Rotation;
        /// <summary>
        /// Rate at which to move Scale.
        /// </summary>
        public float Scale => _moveRates.Scale;
        /// <summary>
        /// Time remaining until the move is complete.
        /// </summary>
        public float TimeRemaining => _moveRates.TimeRemaining;
        /// <summary>
        /// True if position move rate should be instant.
        /// </summary>
        public bool IsPositionInstantValue => _moveRates.IsPositionInstantValue;
        /// <summary>
        /// True if rotation move rate should be instant.
        /// </summary>
        public bool IsRotationInstantValue => _moveRates.IsRotationInstantValue;
        /// <summary>
        /// True if scale move rate should be instant.
        /// </summary>
        public bool IsScaleInstantValue => _moveRates.IsScaleInstantValue;
        /// <summary>
        /// True if any data is set.
        /// </summary>
        public bool IsValid => _moveRates.IsValid;
        public MoveRatesCls(float value) => _moveRates = new(value);
        public MoveRatesCls(float position, float rotation) => _moveRates = new(position, rotation);
        public MoveRatesCls(float position, float rotation, float scale) => _moveRates = new(position, rotation, scale);
        public MoveRatesCls(float position, float rotation, float scale, float timeRemaining) => _moveRates = new(position, rotation, scale, timeRemaining);
        public MoveRatesCls() => _moveRates.ResetState();

        /// <summary>
        /// Sets all rates to instant.
        /// </summary>
        public void SetInstantRates() => _moveRates.SetInstantRates();

        /// <summary>
        /// Sets all rates to the same value.
        /// </summary>
        public void Update(float value) => _moveRates.Update(value);

        /// <summary>
        /// Updates values.
        /// </summary>
        public void Update(float position, float rotation, float scale) => _moveRates.Update(position, rotation, scale);

        /// <summary>
        /// Updates values.
        /// </summary>
        public void Update(float position, float rotation, float scale, float timeRemaining) => _moveRates.Update(position, rotation, scale, timeRemaining);

        /// <summary>
        /// Updaes values.
        /// </summary>
        public void Update(MoveRatesCls mr) => _moveRates.Update(mr.Position, mr.Rotation, mr.Scale);

        /// <summary>
        /// Moves transform to target values.
        /// </summary>
        public void Move(Transform movingTransform, TransformProperties goalProperties, float delta, bool useWorldSpace) => _moveRates.Move(movingTransform, goalProperties, delta, useWorldSpace);

        /// <summary>
        /// Moves transform to target values.
        /// </summary>
        public void Move(Transform movingTransform, TransformProperties goalProperties, TransformPropertiesFlag movedProperties, float delta, bool useWorldSpace) => _moveRates.Move(movingTransform, goalProperties, movedProperties, delta, useWorldSpace);

        public void ResetState() => _moveRates.ResetState();
        public void InitializeState() { }
    }
}