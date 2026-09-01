using FishNet.CodeGenerating;
using FishNet.Serializing;
using UnityEngine;
using UnityEngine.Scripting;

namespace FishNet.Component.Prediction
{
    [UseGlobalCustomSerializer]
    [Preserve]
    public struct RigidbodyState
    {
        public Vector3 Position;
        public Quaternion Rotation;
        [ExcludeSerialization]
        public readonly AutoPackType RotationPacking;
        public bool IsKinematic;
        public Vector3 Velocity;
        public Vector3 AngularVelocity;

        public RigidbodyState(Rigidbody rb, AutoPackType rotationPacking = AutoPackType.Packed)
        {
            Position = rb.transform.localPosition;
            Rotation = rb.transform.localRotation;
            IsKinematic = rb.isKinematic;
            
            #if UNITY_6000_1_OR_NEWER
            Velocity = rb.linearVelocity;
            #else
            Velocity = rb.velocity;
            #endif

            AngularVelocity = rb.angularVelocity;
            RotationPacking = rotationPacking;
        }
    }

    [UseGlobalCustomSerializer]
    [Preserve]
    public struct Rigidbody2DState
    {
        public Vector3 Position;
        public Quaternion Rotation;
        [ExcludeSerialization]
        public readonly AutoPackType RotationPacking;
        public Vector2 Velocity;
        public float AngularVelocity;
        public bool Simulated;
        public bool IsKinematic;

        public Rigidbody2DState(Rigidbody2D rb, AutoPackType rotationPacking = AutoPackType.Packed)
        {
            Position = rb.transform.localPosition;
            Rotation = rb.transform.localRotation;
            RotationPacking = rotationPacking;
            
            #if UNITY_6000_1_OR_NEWER
            Velocity = rb.linearVelocity;
            #else
            Velocity = rb.velocity;
            #endif
            
            AngularVelocity = rb.angularVelocity;
            Simulated = rb.simulated;
            
            #if UNITY_6000_1_OR_NEWER
            IsKinematic = rb.bodyType == RigidbodyType2D.Kinematic;
            #else
            IsKinematic = rb.isKinematic;
            #endif
        }
    }

    [Preserve]
    public static class RigidbodyStateSerializers
    {
        public static void WriteRigidbodyState(this Writer writer, RigidbodyState value)
        {
            writer.WriteVector3(value.Position);

            writer.WriteAutoPackType(value.RotationPacking);
            writer.WriteQuaternion(value.Rotation, value.RotationPacking);

            writer.WriteBoolean(value.IsKinematic);

            if (!value.IsKinematic)
            {
                writer.WriteVector3(value.Velocity);
                writer.WriteVector3(value.AngularVelocity);
            }
        }

        public static RigidbodyState ReadRigidbodyState(this Reader reader)
        {
            Vector3 position = reader.ReadVector3();

            AutoPackType rotationPacking = reader.ReadAutoPackType();
            Quaternion rotation = reader.ReadQuaternion(rotationPacking);

            bool isKinematic = reader.ReadBoolean();

            RigidbodyState state = new()
            {
                Position = position,
                Rotation = rotation,
                IsKinematic = isKinematic,
            };

            if (!state.IsKinematic)
            {
                state.Velocity = reader.ReadVector3();
                state.AngularVelocity = reader.ReadVector3();
            }

            return state;
        }

        public static void WriteRigidbody2DState(this Writer writer, Rigidbody2DState value)
        {
            writer.WriteVector3(value.Position);

            writer.WriteAutoPackType(value.RotationPacking);
            writer.WriteQuaternion(value.Rotation, value.RotationPacking);

            writer.WriteBoolean(value.Simulated);
            writer.WriteBoolean(value.IsKinematic);

            if (value.Simulated)
            {
                writer.WriteVector2(value.Velocity);
                writer.WriteSingle(value.AngularVelocity);
            }
        }

        public static Rigidbody2DState ReadRigidbody2DState(this Reader reader)
        {
            Vector3 position = reader.ReadVector3();

            AutoPackType rotationPacking = reader.ReadAutoPackType();
            Quaternion rotation = reader.ReadQuaternion(rotationPacking);

            bool simulated = reader.ReadBoolean();
            bool isKinematic = reader.ReadBoolean();

            Rigidbody2DState state = new()
            {
                Position = position,
                Rotation = rotation,
                Simulated = simulated,
                IsKinematic = isKinematic,
            };

            if (state.Simulated)
            {
                state.Velocity = reader.ReadVector2();
                state.AngularVelocity = reader.ReadSingle();
            }

            return state;
        }
    }

    [Preserve]
    public static class RigidbodyStateExtensions
    {
        /// <summary>
        /// Gets a RigidbodyState.
        /// </summary>
        public static RigidbodyState GetState(this Rigidbody rb, AutoPackType rotationPacking = AutoPackType.Packed)
        {
            return new(rb, rotationPacking);
        }

        /// <summary>
        /// Sets a state to a rigidbody.
        /// </summary>
        public static void SetState(this Rigidbody rb, RigidbodyState state)
        {
            Transform t = rb.transform;
            t.localPosition = state.Position;
            t.localRotation = state.Rotation;
            rb.isKinematic = state.IsKinematic;

            if (!state.IsKinematic)
            {
                #if UNITY_6000_1_OR_NEWER
                rb.linearVelocity = state.Velocity;
                #else
                rb.velocity = state.Velocity;
                #endif
                rb.angularVelocity = state.AngularVelocity;
            }
        }

        /// <summary>
        /// Gets a Rigidbody2DState.
        /// </summary>
        public static Rigidbody2DState GetState(this Rigidbody2D rb, AutoPackType rotationPacking = AutoPackType.Packed)
        {
            return new(rb, rotationPacking);
        }

        /// <summary>
        /// Sets a state to a rigidbody.
        /// </summary>
        public static void SetState(this Rigidbody2D rb, Rigidbody2DState state)
        {
            Transform t = rb.transform;
            t.position = state.Position;
            t.rotation = state.Rotation;
            #if UNITY_6000_1_OR_NEWER
            rb.bodyType = state.IsKinematic ? RigidbodyType2D.Kinematic : RigidbodyType2D.Dynamic;
            #else
            rb.isKinematic = state.IsKinematic;
            #endif
            if (!state.IsKinematic)
            {
                #if UNITY_6000_1_OR_NEWER
                rb.linearVelocity = state.Velocity;
                #else
                rb.velocity = state.Velocity;
                #endif
                rb.angularVelocity = state.AngularVelocity;
            }
        }
    }
}