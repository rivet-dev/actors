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
using System.Text;
using System.Threading;
using MD = MonoFN.Cecil.Metadata;

namespace MonoFN.Cecil
{
    public sealed class GenericInstanceType : TypeSpecification, IGenericInstance, IGenericContext
    {
        private Collection<TypeReference> arguments;
        public bool HasGenericArguments
        {
            get { return !arguments.IsNullOrEmpty(); }
        }
        public Collection<TypeReference> GenericArguments
        {
            get
            {
                if (arguments == null)
                    Interlocked.CompareExchange(ref arguments, new(), null);

                return arguments;
            }
        }
        public override TypeReference DeclaringType
        {
            get { return ElementType.DeclaringType; }
            set { throw new NotSupportedException(); }
        }
        public override string FullName
        {
            get
            {
                StringBuilder name = new();
                name.Append(base.FullName);
                this.GenericInstanceFullName(name);
                return name.ToString();
            }
        }
        public override bool IsGenericInstance
        {
            get { return true; }
        }
        public override bool ContainsGenericParameter
        {
            get { return this.ContainsGenericParameter() || base.ContainsGenericParameter; }
        }
        IGenericParameterProvider IGenericContext.Type
        {
            get { return ElementType; }
        }

        public GenericInstanceType(TypeReference type) : base(type)
        {
            IsValueType = type.IsValueType;
            etype = MD.ElementType.GenericInst;
        }

        internal GenericInstanceType(TypeReference type, int arity) : this(type)
        {
            arguments = new(arity);
        }
    }
}