//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using MonoFN.Collections.Generic;
using System;
using System.Diagnostics;
using System.Threading;

namespace MonoFN.Cecil
{
    public struct CustomAttributeArgument
    {
        public TypeReference Type { get; }
        public object Value { get; }

        public CustomAttributeArgument(TypeReference type, object value)
        {
            Mixin.CheckType(type);
            this.Type = type;
            this.Value = value;
        }
    }

    public struct CustomAttributeNamedArgument
    {
        public string Name { get; }
        public CustomAttributeArgument Argument { get; }

        public CustomAttributeNamedArgument(string name, CustomAttributeArgument argument)
        {
            Mixin.CheckName(name);
            this.Name = name;
            this.Argument = argument;
        }
    }

    public interface ICustomAttribute
    {
        TypeReference AttributeType { get; }
        bool HasFields { get; }
        bool HasProperties { get; }
        bool HasConstructorArguments { get; }
        Collection<CustomAttributeNamedArgument> Fields { get; }
        Collection<CustomAttributeNamedArgument> Properties { get; }
        Collection<CustomAttributeArgument> ConstructorArguments { get; }
    }

    [DebuggerDisplay("{AttributeType}")]
    public sealed class CustomAttribute : ICustomAttribute
    {
        internal CustomAttributeValueProjection projection;
        internal readonly uint signature;
        internal bool resolved;
        private byte[] blob;
        internal Collection<CustomAttributeArgument> arguments;
        internal Collection<CustomAttributeNamedArgument> fields;
        internal Collection<CustomAttributeNamedArgument> properties;
        public MethodReference Constructor { get; set; }
        public TypeReference AttributeType
        {
            get { return Constructor.DeclaringType; }
        }
        public bool IsResolved
        {
            get { return resolved; }
        }
        public bool HasConstructorArguments
        {
            get
            {
                Resolve();

                return !arguments.IsNullOrEmpty();
            }
        }
        public Collection<CustomAttributeArgument> ConstructorArguments
        {
            get
            {
                Resolve();

                if (arguments == null)
                    Interlocked.CompareExchange(ref arguments, new(), null);

                return arguments;
            }
        }
        public bool HasFields
        {
            get
            {
                Resolve();

                return !fields.IsNullOrEmpty();
            }
        }
        public Collection<CustomAttributeNamedArgument> Fields
        {
            get
            {
                Resolve();

                if (fields == null)
                    Interlocked.CompareExchange(ref fields, new(), null);

                return fields;
            }
        }
        public bool HasProperties
        {
            get
            {
                Resolve();

                return !properties.IsNullOrEmpty();
            }
        }
        public Collection<CustomAttributeNamedArgument> Properties
        {
            get
            {
                Resolve();

                if (properties == null)
                    Interlocked.CompareExchange(ref properties, new(), null);

                return properties;
            }
        }
        internal bool HasImage
        {
            get { return Constructor != null && Constructor.HasImage; }
        }
        internal ModuleDefinition Module
        {
            get { return Constructor.Module; }
        }

        internal CustomAttribute(uint signature, MethodReference constructor)
        {
            this.signature = signature;
            this.Constructor = constructor;
            resolved = false;
        }

        public CustomAttribute(MethodReference constructor)
        {
            this.Constructor = constructor;
            resolved = true;
        }

        public CustomAttribute(MethodReference constructor, byte[] blob)
        {
            this.Constructor = constructor;
            resolved = false;
            this.blob = blob;
        }

        public byte[] GetBlob()
        {
            if (blob != null)
                return blob;

            if (!HasImage)
                throw new NotSupportedException();

            return Module.Read(ref blob, this, (attribute, reader) => reader.ReadCustomAttributeBlob(attribute.signature));
        }

        private void Resolve()
        {
            if (resolved || !HasImage)
                return;

            lock (Module.SyncRoot)
            {
                if (resolved)
                    return;

                Module.Read(this, (attribute, reader) =>
                {
                    try
                    {
                        reader.ReadCustomAttributeSignature(attribute);
                        resolved = true;
                    }
                    catch (ResolutionException)
                    {
                        if (arguments != null)
                            arguments.Clear();
                        if (fields != null)
                            fields.Clear();
                        if (properties != null)
                            properties.Clear();

                        resolved = false;
                    }
                });
            }
        }
    }
}