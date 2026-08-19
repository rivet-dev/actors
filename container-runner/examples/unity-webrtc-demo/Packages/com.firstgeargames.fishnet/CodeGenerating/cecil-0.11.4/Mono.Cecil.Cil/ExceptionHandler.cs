//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

namespace MonoFN.Cecil.Cil
{
    public enum ExceptionHandlerType
    {
        Catch = 0,
        Filter = 1,
        Finally = 2,
        Fault = 4
    }

    public sealed class ExceptionHandler
    {
        public Instruction TryStart { get; set; }
        public Instruction TryEnd { get; set; }
        public Instruction FilterStart { get; set; }
        public Instruction HandlerStart { get; set; }
        public Instruction HandlerEnd { get; set; }
        public TypeReference CatchType { get; set; }
        public ExceptionHandlerType HandlerType { get; set; }

        public ExceptionHandler(ExceptionHandlerType handlerType)
        {
            HandlerType = handlerType;
        }
    }
}