using System;
using GameKit.Dependencies.Utilities;
using UnityEngine;

namespace FishNet.Object
{
    public static class TransformPropertiesExtensions
    {
        /// <summary>
        /// Creates direction between two TransformProperties.
        /// </summary>
        /// <param name = "divisor">Value to divide results by.</param>
        /// <returns></returns>
        public static TransformProperties CreateDirections(this TransformProperties prevProperties, TransformProperties nextProperties, uint divisor = 1)
        {
            Vector3 position = (nextProperties.Position - prevProperties.Position) / divisor;

            Quaternion rotation = nextProperties.Rotation.Subtract(prevProperties.Rotation);
            //If more than 1 tick span then get a portion of the rotation.
            if (divisor > 1)
            {
                float percent = 1f / (float)divisor;
                rotation = Quaternion.Lerp(Quaternion.identity, nextProperties.Rotation, percent);
            }

            Vector3 scale = (nextProperties.Scale - prevProperties.Scale) / divisor;

            return new(position, rotation, scale);
        }

        /// <summary>
        /// Sets values of TransformPropertiesCls to a transforms world properties.
        /// </summary>
        public static void SetWorldProperties(this TransformPropertiesCls tp, Transform t)
        {
            tp.Position = t.position;
            tp.Rotation = t.rotation;
            tp.LocalScale = t.localScale;
        }

        /// <summary>
        /// Sets values of TransformPropertiesCls to a transforms world properties.
        /// </summary>
        public static void SetWorldProperties(this TransformProperties tp, Transform t)
        {
            tp.Position = t.position;
            tp.Rotation = t.rotation;
            tp.Scale = t.localScale;
        }
    }

    [Serializable]
    public class TransformPropertiesCls : IResettable
    {
        public Vector3 Position;
        public Quaternion Rotation;
        public Vector3 LocalScale;
        public TransformPropertiesCls() { }

        public TransformPropertiesCls(Vector3 position, Quaternion rotation, Vector3 localScale)
        {
            Position = position;
            Rotation = rotation;
            LocalScale = localScale;
        }

        public void InitializeState() { }

        public void ResetState()
        {
            Update(Vector3.zero, Quaternion.identity, Vector3.zero);
        }

        public void Update(Transform t)
        {
            Update(t.position, t.rotation, t.localScale);
        }

        public void Update(TransformPropertiesCls tp)
        {
            Update(tp.Position, tp.Rotation, tp.LocalScale);
        }

        public void Update(TransformProperties tp)
        {
            Update(tp.Position, tp.Rotation, tp.Scale);
        }

        public void Update(Vector3 position, Quaternion rotation)
        {
            Update(position, rotation, LocalScale);
        }

        public void Update(Vector3 position, Quaternion rotation, Vector3 localScale)
        {
            Position = position;
            Rotation = rotation;
            LocalScale = localScale;
        }

        /// <summary>
        /// Returns if this TransformProperties equals anothers values.
        /// </summary>
        public bool ValuesEquals(TransformPropertiesCls properties)
        {
            return Position == properties.Position && Rotation == properties.Rotation && LocalScale == properties.LocalScale;
        }

        /// <summary>
        /// Returns this classes values as the struct version of TransformProperties.
        /// </summary>
        /// <returns></returns>
        public TransformProperties ToStruct()
        {
            TransformProperties result = new(Position, Rotation, LocalScale);
            return result;
        }
    }

    [Serializable]
    public struct TransformProperties
    {
        public Vector3 Position;
        public Quaternion Rotation;
        [Obsolete("Use Scale.")] //Remove V5
        public Vector3 LocalScale => Scale;
        public Vector3 Scale;
        /// <summary>
        /// Becomes true when values are set through update or constructor.
        /// </summary>
        public bool IsValid;

        public TransformProperties(Vector3 position, Quaternion rotation, Vector3 localScale)
        {
            Position = position;
            Rotation = rotation;
            Scale = localScale;
            IsValid = true;
        }

        /// <summary>
        /// Creates a TransformProperties with default position and rotation, with Vector3.one scale.
        /// </summary>
        public static TransformProperties GetTransformDefault() => new(Vector3.zero, Quaternion.identity, Vector3.one);

        public override string ToString()
        {
            return $"Position: {Position.ToString()}, Rotation {Rotation.ToString()}, Scale {Scale.ToString()}";
        }

        public TransformProperties(Transform t) : this(t.position, t.rotation, t.localScale) { }

        [Obsolete("Use ResetState.")]
        public void Reset() => ResetState();

        public void ResetState()
        {
            Update(Vector3.zero, Quaternion.identity, Vector3.zero);
            IsValid = false;
        }

        public void Update(Transform t)
        {
            Update(t.position, t.rotation, t.localScale);
        }

        public void Update(TransformProperties tp)
        {
            Update(tp.Position, tp.Rotation, tp.Scale);
        }

        public void Update(Vector3 position, Quaternion rotation)
        {
            Update(position, rotation, Scale);
        }

        public void Update(Vector3 position, Quaternion rotation, Vector3 localScale)
        {
            Position = position;
            Rotation = rotation;
            Scale = localScale;
            IsValid = true;
        }

        /// <summary>
        /// Adds another transformProperties onto this.
        /// </summary>
        /// <param name = "tp"></param>
        public void Add(TransformProperties tp)
        {
            Position += tp.Position;
            Rotation *= tp.Rotation;
            Scale += tp.Scale;
        }

        /// <summary>
        /// Subtracts another transformProperties from this.
        /// </summary>
        /// <param name = "tp"></param>
        public void Subtract(TransformProperties tp)
        {
            Position -= tp.Position;
            Rotation *= Quaternion.Inverse(tp.Rotation);
            Scale -= tp.Scale;
        }

        /// <summary>
        /// Returns if this TransformProperties equals anothers values.
        /// </summary>
        public bool ValuesEquals(TransformProperties properties)
        {
            return Position == properties.Position && Rotation == properties.Rotation && Scale == properties.Scale;
        }
    }
}